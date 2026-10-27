use std::collections::BTreeSet;
use unicode_security::MixedScript;
use unicode_security::confusable_detection::skeleton;

pub const MATCH_FIELD: &str = "nv:match";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub explicit_port: bool,
    pub site: String,
    pub exact_only: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rule {
    #[default]
    Site,
    Host,
    Exact,
    Never,
}

impl Rule {
    pub fn parse(s: &str) -> Rule {
        match s.trim().to_ascii_lowercase().as_str() {
            "host" => Rule::Host,
            "exact" => Rule::Exact,
            "never" => Rule::Never,
            _ => Rule::Site,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quality {
    Host,
    Site,
}

impl Quality {
    pub fn as_str(self) -> &'static str {
        match self {
            Quality::Host => "host",
            Quality::Site => "site",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub quality: Quality,
    pub insecure: bool,
}

fn default_port(scheme: &str) -> u16 {
    if scheme == "http" { 80 } else { 443 }
}

fn is_ip(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
}

pub fn registrable(host: &str) -> Option<String> {
    let h = host.trim_end_matches('.');
    if h.is_empty() || is_ip(h) || h == "localhost" || h.ends_with(".localhost") || !h.contains('.') {
        return None;
    }
    let d = psl::domain(h.as_bytes())?;
    std::str::from_utf8(d.as_bytes()).ok().map(str::to_owned)
}

fn from_url(u: &url::Url) -> Option<Origin> {
    let scheme = u.scheme();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = match u.host()? {
        url::Host::Domain(d) => d.trim_end_matches('.').to_ascii_lowercase(),
        url::Host::Ipv4(a) => a.to_string(),
        url::Host::Ipv6(a) => a.to_string(),
    };
    if host.is_empty() {
        return None;
    }
    let exact_only = is_ip(&host) || host == "localhost" || host.ends_with(".localhost");
    let site = if exact_only { host.clone() } else { registrable(&host).unwrap_or_else(|| host.clone()) };
    Some(Origin {
        scheme: scheme.to_owned(),
        port: u.port().unwrap_or_else(|| default_port(scheme)),
        explicit_port: u.port().is_some(),
        host,
        site,
        exact_only,
    })
}

pub fn parse_page(s: &str) -> Option<Origin> {
    let s = s.trim();
    if s.is_empty() || s == "null" {
        return None;
    }
    from_url(&url::Url::parse(s).ok()?)
}

pub fn parse_entry(s: &str) -> Option<Origin> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    match url::Url::parse(s) {
        Ok(u) if u.has_host() => from_url(&u),
        _ if !s.contains("://") => from_url(&url::Url::parse(&format!("https://{s}")).ok()?),
        _ => None,
    }
}

pub fn match_url(entry: &str, page: &Origin, rule: Rule) -> Option<Hit> {
    if rule == Rule::Never {
        return None;
    }
    let e = parse_entry(entry)?;
    if e.explicit_port && e.port != page.port {
        return None;
    }
    let same_host = e.host == page.host;
    let quality = if same_host {
        Quality::Host
    } else if rule == Rule::Site && !e.exact_only && !page.exact_only && e.site == page.site {
        Quality::Site
    } else {
        return None;
    };
    if rule == Rule::Exact && (e.scheme != page.scheme || e.port != page.port) {
        return None;
    }
    let insecure = e.scheme == "https" && page.scheme == "http";
    Some(Hit { quality, insecure })
}

pub fn match_entry(urls: &[String], page: &Origin, rule: Rule) -> Option<Hit> {
    urls.iter().filter_map(|u| match_url(u, page, rule)).min_by_key(|h| (h.insecure, h.quality))
}

pub fn unicode_host(host: &str) -> String {
    let (u, r) = idna::domain_to_unicode(host);
    if r.is_ok() { u } else { host.to_owned() }
}

fn edit_distance(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, c) in d[0].iter_mut().enumerate() {
        *c = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
}

fn label_and_suffix(site: &str) -> (&str, &str) {
    site.split_once('.').unwrap_or((site, ""))
}

fn skel(s: &str) -> String {
    skeleton(&s.to_lowercase()).collect::<String>().to_lowercase()
}

pub fn imitates(page_site: &str, known: &str) -> bool {
    if page_site == known {
        return false;
    }
    let pu = unicode_host(page_site);
    let ku = unicode_host(known);
    let (pl, ps) = label_and_suffix(&pu);
    let (kl, ks) = label_and_suffix(&ku);
    if skel(&pu) == skel(&ku) {
        return true;
    }
    if ps != ks {
        return false;
    }
    let pc: Vec<char> = skel(pl).chars().collect();
    let kc: Vec<char> = skel(kl).chars().collect();
    let close = edit_distance(&pc, &kc) <= 1;
    (close && kl.chars().count() >= 6) || (close && !pl.is_single_script())
}

pub fn lookalike<'a>(page: &Origin, known_sites: impl IntoIterator<Item = &'a str>) -> Option<String> {
    if page.exact_only {
        return None;
    }
    let set: BTreeSet<&str> = known_sites.into_iter().collect();
    if set.contains(page.site.as_str()) {
        return None;
    }
    set.into_iter().find(|k| imitates(&page.site, k)).map(str::to_owned)
}

pub fn sites_of(urls: &[String]) -> impl Iterator<Item = String> + '_ {
    urls.iter().filter_map(|u| parse_entry(u)).filter(|o| !o.exact_only).map(|o| o.site)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(s: &str) -> Origin {
        parse_page(s).unwrap_or_else(|| panic!("page {s}"))
    }
    fn hit(entry: &str, p: &str) -> Option<Hit> {
        match_url(entry, &page(p), Rule::Site)
    }
    const HOST: Option<Hit> = Some(Hit { quality: Quality::Host, insecure: false });
    const SITE: Option<Hit> = Some(Hit { quality: Quality::Site, insecure: false });

    #[test]
    fn registrable_domains_follow_the_psl() {
        assert_eq!(registrable("www.example.com").as_deref(), Some("example.com"));
        assert_eq!(registrable("a.b.example.co.uk").as_deref(), Some("example.co.uk"));
        assert_eq!(registrable("mario.github.io").as_deref(), Some("mario.github.io"));
        assert_eq!(registrable("foo.blogspot.com").as_deref(), Some("foo.blogspot.com"));
        assert_eq!(registrable("comune.roma.it").as_deref(), Some("comune.roma.it"));
        assert_eq!(registrable("localhost"), None);
        assert_eq!(registrable("192.168.1.1"), None);
        assert_eq!(registrable("intranet"), None);
    }

    #[test]
    fn corpus() {
        let cases: &[(&str, &str, Option<Hit>)] = &[
            ("https://example.com", "https://example.com", HOST),
            ("https://example.com/login?x=1", "https://example.com", HOST),
            ("example.com", "https://example.com", HOST),
            ("example.com/login", "https://www.example.com", SITE),
            ("https://www.example.com", "https://login.example.com", SITE),
            ("https://EXAMPLE.com", "https://example.com", HOST),
            ("https://example.com.", "https://example.com", HOST),
            ("https://example.com", "https://example.org", None),
            ("https://example.com", "https://example.com.evil.net", None),
            ("https://example.com", "https://notexample.com", None),
            ("https://evil-example.com", "https://example.com", None),
            ("https://alice.github.io", "https://bob.github.io", None),
            ("https://alice.github.io/app", "https://alice.github.io", HOST),
            ("https://shop.example.co.uk", "https://example.co.uk", SITE),
            ("https://example.co.uk", "https://other.co.uk", None),
            ("https://example.com", "http://example.com", Some(Hit { quality: Quality::Host, insecure: true })),
            ("http://example.com", "https://example.com", HOST),
            ("ftp://example.com", "https://example.com", None),
            ("android://abc@com.example.app/", "https://example.com", None),
            ("https://example.com:8443", "https://example.com:8443", HOST),
            ("https://example.com:8443", "https://example.com", None),
            ("example.com:8443", "https://example.com:8443", HOST),
            ("https://example.com", "https://example.com:8443", HOST),
            ("https://192.168.1.10", "https://192.168.1.10", HOST),
            ("https://192.168.1.10", "https://192.168.1.11", None),
            ("http://localhost:3000", "http://localhost:3000", HOST),
            ("http://localhost:3000", "http://localhost:3001", None),
            ("http://localhost", "http://app.localhost", None),
            ("https://[::1]:8443", "https://[::1]:8443", HOST),
            ("https://bücher.de", "https://xn--bcher-kva.de", HOST),
            ("https://www.xn--bcher-kva.de", "https://bücher.de", SITE),
            ("", "https://example.com", None),
            ("not a url at all", "https://example.com", None),
        ];
        for (e, p, want) in cases {
            assert_eq!(hit(e, p), *want, "entry {e} on page {p}");
        }
    }

    #[test]
    fn pages_that_never_get_anything() {
        for p in ["null", "", "data:text/html,hi", "about:blank", "chrome://settings", "moz-extension://abc/popup.html", "chrome-extension://abc/x.html", "file:///etc/passwd", "javascript:alert(1)"] {
            assert!(parse_page(p).is_none(), "{p}");
        }
    }

    #[test]
    fn rules_narrow_the_match() {
        let p = page("https://login.example.com");
        let urls = vec!["https://www.example.com".to_owned()];
        assert_eq!(match_entry(&urls, &p, Rule::Site), SITE);
        assert_eq!(match_entry(&urls, &p, Rule::Host), None);
        assert_eq!(match_entry(&urls, &p, Rule::Never), None);
        let same = vec!["https://login.example.com".to_owned()];
        assert_eq!(match_entry(&same, &p, Rule::Host), HOST);
        assert_eq!(match_entry(&same, &p, Rule::Exact), HOST);
        assert_eq!(match_entry(&same, &page("http://login.example.com"), Rule::Exact), None);
        assert_eq!(Rule::parse(" Never "), Rule::Never);
        assert_eq!(Rule::parse("whatever"), Rule::Site);
    }

    #[test]
    fn best_url_wins() {
        let urls = vec!["https://example.com".to_owned(), "https://login.example.com".to_owned(), "https://other.org".to_owned()];
        assert_eq!(match_entry(&urls, &page("https://login.example.com"), Rule::Site), HOST);
        let mixed = vec!["https://login.example.com".to_owned(), "http://www.example.com".to_owned()];
        assert_eq!(match_entry(&mixed, &page("http://login.example.com"), Rule::Site), SITE);
    }

    #[test]
    fn lookalikes() {
        let known = ["paypal.com", "google.com", "microsoft.com", "example.com", "intesasanpaolo.com", "ing.it"];
        let la = |p: &str| lookalike(&page(p), known.iter().copied());
        assert_eq!(la("https://www.paypa1.com").as_deref(), Some("paypal.com"));
        assert_eq!(la("https://paypall.com").as_deref(), Some("paypal.com"));
        assert_eq!(la("https://rnicrosoft.com").as_deref(), Some("microsoft.com"));
        assert_eq!(la("https://gooogle.com").as_deref(), Some("google.com"));
        assert_eq!(la("https://intesasanpaol0.com").as_deref(), Some("intesasanpaolo.com"));
        assert_eq!(la("https://p\u{430}ypal.com").as_deref(), Some("paypal.com"));
        assert_eq!(la("https://xn--pypal-4ve.com").as_deref(), Some("paypal.com"));
        assert_eq!(la("https://www.paypal.com"), None);
        assert_eq!(la("https://wikipedia.org"), None);
        assert_eq!(la("https://ing.com"), None);
        assert_eq!(la("https://img.it"), None);
        assert_eq!(la("https://examples.org"), None);
        assert_eq!(la("http://192.168.0.1"), None);
    }

    #[test]
    fn unicode_display() {
        assert_eq!(unicode_host("xn--bcher-kva.de"), "bücher.de");
        assert_eq!(unicode_host("example.com"), "example.com");
    }

    #[test]
    fn edit_distance_counts_transpositions_once() {
        let c = |s: &str| s.chars().collect::<Vec<_>>();
        assert_eq!(edit_distance(&c("paypal"), &c("papyal")), 1);
        assert_eq!(edit_distance(&c("paypal"), &c("paypal")), 0);
        assert_eq!(edit_distance(&c("paypal"), &c("pay")), 3);
    }
}
