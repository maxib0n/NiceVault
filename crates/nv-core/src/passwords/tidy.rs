use super::totp::Totp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Known {
    Urls,
    Created,
    Updated,
    Totp,
    Hide,
    Noise,
}

pub fn norm(name: &str) -> String {
    name.trim().trim_start_matches('\u{feff}').to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

pub fn classify(name: &str) -> Option<Known> {
    let n = norm(name);
    Some(match n.as_str() {
        "autofillurls" | "autofillurl" | "urls" | "uris" | "websites" | "loginuris" | "additionalurls" | "additionalurl"
        | "matchurl" | "otherurls" | "alternateurls" => Known::Urls,
        "createtime" | "creationtime" | "created" | "createdat" | "creationdate" | "timecreated" | "datecreated"
        | "createddate" | "createdtime" | "creation" => Known::Created,
        "modifytime" | "modificationtime" | "modified" | "lastmodified" | "modifiedat" | "updatedat" | "updated"
        | "updatetime" | "lastupdated" | "datemodified" | "modifieddate" | "revisiondate" | "lastmodifiedtime"
        | "timepasswordchanged" | "passwordchanged" | "passwordmodified" => Known::Updated,
        "otpsecret" | "totpsecret" | "otpurl" | "otpuri" | "totpuri" | "authenticatorkey" | "twofactorsecret" => Known::Totp,
        "cardnumber" | "cardnum" | "cvc" | "cvv" | "cvv2" | "securitycode" | "pin" | "pincode" | "cardpin" => Known::Hide,
        "icon" | "iconid" | "mode" | "keepasshttpsettings" | "keepassxcbrowsersettings" => Known::Noise,
        _ if n.starts_with("etm") && name.trim_start().starts_with("_etm_") => Known::Noise,
        _ => return None,
    })
}

pub fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let ms = if let Ok(n) = s.parse::<i64>() {
        match n {
            n if n <= 0 => return None,
            n if n < 100_000_000_000 => n.checked_mul(1000)?,
            n if n < 100_000_000_000_000 => n,
            n if n < 100_000_000_000_000_000 => n / 1000,
            n => n / 1_000_000,
        }
    } else if let Ok(f) = s.parse::<f64>() {
        if !(f.is_finite() && f > 0.0 && f < 1e11) {
            return None;
        }
        (f * 1000.0) as i64
    } else if s.len() == 10 && s.as_bytes().get(4) == Some(&b'-') {
        super::import::ms_of_rfc3339(&format!("{s}T00:00:00Z"))?
    } else {
        let b = s.as_bytes();
        if b.len() < 19 || b.get(4) != Some(&b'-') || !matches!(b.get(10), Some(b'T' | b't' | b' ')) {
            return None;
        }
        super::import::ms_of_rfc3339(s)?
    };
    (631_152_000_000..7_258_118_400_000).contains(&ms).then_some(ms)
}

pub fn parse_urls(s: &str) -> Option<Vec<String>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut push = |u: &str| {
        let u = u.trim();
        if !u.is_empty() && !out.iter().any(|o| o == u) {
            out.push(u.to_owned());
        }
    };
    if s.starts_with('[') || s.starts_with('"') {
        match serde_json::from_str::<serde_json::Value>(s).ok()? {
            serde_json::Value::Array(a) => {
                for v in a {
                    match v {
                        serde_json::Value::String(u) => push(&u),
                        serde_json::Value::Object(o) => {
                            let u = ["url", "uri", "href", "value"].iter().find_map(|k| o.get(*k).and_then(|v| v.as_str()));
                            push(u?);
                        }
                        _ => return None,
                    }
                }
            }
            serde_json::Value::String(u) => push(&u),
            _ => return None,
        }
    } else {
        for u in s.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|u| !u.is_empty()) {
            let l = u.to_ascii_lowercase();
            if !(l.starts_with("https://") || l.starts_with("http://")) {
                return None;
            }
            push(u);
        }
    }
    Some(out)
}

#[derive(Debug, Default)]
pub struct Found {
    pub urls: Vec<String>,
    pub created: Option<i64>,
    pub updated: Option<i64>,
    pub totp: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Keep,
    Hide,
    Drop,
}

pub fn field(name: &str, value: &str, has_totp: bool, found: &mut Found) -> Act {
    let Some(k) = classify(name) else { return Act::Keep };
    let v = value.trim();
    match k {
        Known::Urls => match parse_urls(v) {
            Some(u) => {
                for x in u {
                    if !found.urls.contains(&x) {
                        found.urls.push(x);
                    }
                }
                Act::Drop
            }
            None => Act::Keep,
        },
        Known::Created | Known::Updated => match parse_time(v) {
            Some(ms) => {
                let slot = if k == Known::Created { &mut found.created } else { &mut found.updated };
                *slot = Some(slot.map_or(ms, |o| if k == Known::Created { o.min(ms) } else { o.max(ms) }));
                Act::Drop
            }
            None if v.is_empty() => Act::Drop,
            None => Act::Keep,
        },
        Known::Totp if !has_totp && found.totp.is_none() && Totp::parse(v).is_ok() => {
            found.totp = Some(v.to_owned());
            Act::Drop
        }
        Known::Totp => Act::Hide,
        Known::Hide => Act::Hide,
        Known::Noise => {
            let numbers_only = matches!(norm(name).as_str(), "icon" | "iconid" | "mode");
            if !numbers_only || v.chars().all(|c| c.is_ascii_digit()) { Act::Drop } else { Act::Keep }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(classify("AUTOFILLURLS"), Some(Known::Urls));
        assert_eq!(classify("createTime"), Some(Known::Created));
        assert_eq!(classify("Last Modified"), Some(Known::Updated));
        assert_eq!(classify("otpSecret"), Some(Known::Totp));
        assert_eq!(classify("CVV"), Some(Known::Hide));
        assert_eq!(classify("_etm_template_uuid"), Some(Known::Noise));
        assert_eq!(classify("etm"), None);
        assert_eq!(classify("Security question"), None);
        assert_eq!(classify("ID"), None, "an id may be the person's own data");
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("1774863307"), Some(1_774_863_307_000));
        assert_eq!(parse_time("1774863307123"), Some(1_774_863_307_123));
        assert_eq!(parse_time("1774863307123456"), Some(1_774_863_307_123));
        assert_eq!(parse_time("1774863307.5"), Some(1_774_863_307_500));
        assert_eq!(parse_time("2024-02-29T12:00:00Z"), Some(1_709_208_000_000));
        assert_eq!(parse_time("2024-02-29 12:00:00"), Some(1_709_208_000_000));
        assert_eq!(parse_time("2024-02-29"), Some(1_709_164_800_000));
        assert_eq!(parse_time("0"), None);
        assert_eq!(parse_time("42"), None, "1970 is not a real creation date");
        assert_eq!(parse_time("ieri"), None);
    }

    #[test]
    fn urls() {
        assert_eq!(
            parse_urls(r#"[{"url":"https://login.microsoftonline.com/","mode":0},{"url":"https://b.example"}]"#),
            Some(vec!["https://login.microsoftonline.com/".to_owned(), "https://b.example".to_owned()])
        );
        assert_eq!(parse_urls(r#"["https://a.example","https://a.example"]"#), Some(vec!["https://a.example".to_owned()]));
        assert_eq!(parse_urls("https://a.example, http://b.example"), Some(vec!["https://a.example".to_owned(), "http://b.example".to_owned()]));
        assert_eq!(parse_urls("non un url"), None);
        assert_eq!(parse_urls("[1,2]"), None);
    }

    #[test]
    fn fields() {
        let mut f = Found::default();
        assert_eq!(field("AUTOFILLURLS", r#"[{"url":"https://x.example/","mode":0}]"#, false, &mut f), Act::Drop);
        assert_eq!(field("CREATETIME", "1774863307", false, &mut f), Act::Drop);
        assert_eq!(field("MODIFYTIME", "1790593026", false, &mut f), Act::Drop);
        assert_eq!(field("otpSecret", "JBSWY3DPEHPK3PXP", false, &mut f), Act::Drop);
        assert_eq!(field("otpSecret", "JBSWY3DPEHPK3PXP", false, &mut f), Act::Hide, "only one TOTP");
        assert_eq!(field("cvc", "123", false, &mut f), Act::Hide);
        assert_eq!(field("Icon", "0", false, &mut f), Act::Drop);
        assert_eq!(field("Icon", "una bandiera", false, &mut f), Act::Keep);
        assert_eq!(field("created", "boh", false, &mut f), Act::Keep);
        assert_eq!(field("Domanda", "x", false, &mut f), Act::Keep);
        assert_eq!(f.urls, ["https://x.example/"]);
        assert_eq!((f.created, f.updated), (Some(1_774_863_307_000), Some(1_790_593_026_000)));
        assert_eq!(f.totp.as_deref(), Some("JBSWY3DPEHPK3PXP"));
    }
}
