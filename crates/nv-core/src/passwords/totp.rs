use hmac::{Hmac, Mac};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().replace(['-', '_'], "").trim_start_matches("HMAC") {
            "SHA1" => Some(Self::Sha1),
            "SHA256" => Some(Self::Sha256),
            "SHA512" => Some(Self::Sha512),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha512 => "SHA512",
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TotpError {
    #[error("not a TOTP secret or otpauth://totp URI")]
    Invalid,
    #[error("only time-based codes (otpauth://totp) are supported")]
    Unsupported,
    #[error("secret is not valid base32")]
    Secret,
    #[error("digits must be 6..=8 and period 1..=300 seconds")]
    Params,
}

#[derive(Clone)]
pub struct Totp {
    secret: Zeroizing<Vec<u8>>,
    pub algorithm: Algorithm,
    pub digits: u32,
    pub period: u64,
    pub issuer: Option<String>,
    pub account: Option<String>,
}

impl std::fmt::Debug for Totp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Totp").field("algorithm", &self.algorithm).field("digits", &self.digits).field("period", &self.period).finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TotpCode {
    pub code: String,
    pub remaining: u64,
    pub period: u64,
    pub digits: u32,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TotpInfo {
    pub issuer: Option<String>,
    pub account: Option<String>,
    pub algorithm: Algorithm,
    pub digits: u32,
    pub period: u64,
}

pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let (mut buf, mut bits) = (0u64, 0u32);
    let mut n = 0;
    for c in s.chars() {
        let v = match c.to_ascii_uppercase() {
            c @ 'A'..='Z' => c as u64 - 'A' as u64,
            c @ '2'..='7' => c as u64 - '2' as u64 + 26,
            ' ' | '-' | '=' | '\t' | '\n' | '\r' => continue,
            _ => return None,
        };
        n += 1;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    (n > 0).then_some(out)
}

pub fn base32_encode(b: &[u8]) -> String {
    const A: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let (mut buf, mut bits) = (0u64, 0u32);
    for &x in b {
        buf = (buf << 8) | x as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(A[((buf >> bits) & 31) as usize] as char);
        }
        buf &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(A[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(v) => {
                    out.push(v);
                    i += 3;
                    continue;
                }
                None => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || b"-._~".contains(&c) {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

impl Totp {
    pub fn new(secret: Vec<u8>, algorithm: Algorithm, digits: u32, period: u64) -> Result<Totp, TotpError> {
        if !(6..=8).contains(&digits) || !(1..=300).contains(&period) {
            return Err(TotpError::Params);
        }
        if secret.is_empty() {
            return Err(TotpError::Secret);
        }
        Ok(Totp { secret: Zeroizing::new(secret), algorithm, digits, period, issuer: None, account: None })
    }

    pub fn parse(input: &str) -> Result<Totp, TotpError> {
        let s = input.trim();
        let lower = s.get(..10).map(|p| p.to_ascii_lowercase()).unwrap_or_default();
        if !lower.starts_with("otpauth:") {
            if s.contains("://") || s.contains(':') {
                return Err(if s.to_ascii_lowercase().starts_with("otpauth-migration:") { TotpError::Unsupported } else { TotpError::Invalid });
            }
            let secret = base32_decode(s).ok_or(TotpError::Secret)?;
            return Totp::new(secret, Algorithm::Sha1, 6, 30);
        }
        let rest = s["otpauth:".len()..].trim_start_matches('/');
        let (kind, rest) = rest.split_once('/').ok_or(TotpError::Invalid)?;
        if !kind.eq_ignore_ascii_case("totp") {
            return Err(TotpError::Unsupported);
        }
        let (label, query) = rest.split_once('?').unwrap_or((rest, ""));
        let label = pct_decode(label);
        let (mut issuer, account) = match label.split_once(':') {
            Some((i, a)) => (Some(i.trim().to_owned()), a.trim().to_owned()),
            None => (None, label.trim().to_owned()),
        };
        let (mut secret, mut alg, mut digits, mut period) = (None, Algorithm::Sha1, 6, 30);
        for kv in query.split('&').filter(|kv| !kv.is_empty()) {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            let v = pct_decode(v);
            match k.to_ascii_lowercase().as_str() {
                "secret" => secret = Some(base32_decode(&v).ok_or(TotpError::Secret)?),
                "issuer" if !v.trim().is_empty() => issuer = Some(v.trim().to_owned()),
                "algorithm" => alg = Algorithm::parse(&v).ok_or(TotpError::Params)?,
                "digits" => digits = v.trim().parse().map_err(|_| TotpError::Params)?,
                "period" => period = v.trim().parse().map_err(|_| TotpError::Params)?,
                _ => {}
            }
        }
        let mut t = Totp::new(secret.ok_or(TotpError::Secret)?, alg, digits, period)?;
        t.issuer = issuer.filter(|i| !i.is_empty());
        t.account = Some(account).filter(|a| !a.is_empty());
        Ok(t)
    }

    pub fn to_uri(&self) -> Zeroizing<String> {
        let label = match (&self.issuer, &self.account) {
            (Some(i), Some(a)) => format!("{}:{}", pct_encode(i), pct_encode(a)),
            (Some(i), None) => pct_encode(i),
            (None, Some(a)) => pct_encode(a),
            (None, None) => String::new(),
        };
        let mut uri = format!("otpauth://totp/{label}?secret={}", base32_encode(&self.secret));
        if let Some(i) = &self.issuer {
            uri.push_str(&format!("&issuer={}", pct_encode(i)));
        }
        uri.push_str(&format!("&algorithm={}&digits={}&period={}", self.algorithm.name(), self.digits, self.period));
        Zeroizing::new(uri)
    }

    pub fn info(&self) -> TotpInfo {
        TotpInfo { issuer: self.issuer.clone(), account: self.account.clone(), algorithm: self.algorithm, digits: self.digits, period: self.period }
    }

    pub fn code_at(&self, t: u64) -> String {
        let counter = (t / self.period).to_be_bytes();
        let mac: Zeroizing<Vec<u8>> = Zeroizing::new(match self.algorithm {
            Algorithm::Sha1 => hmac_of::<Hmac<sha1::Sha1>>(&self.secret, &counter),
            Algorithm::Sha256 => hmac_of::<Hmac<sha2::Sha256>>(&self.secret, &counter),
            Algorithm::Sha512 => hmac_of::<Hmac<sha2::Sha512>>(&self.secret, &counter),
        });
        let off = (mac[mac.len() - 1] & 0x0f) as usize;
        let bin = u32::from_be_bytes([mac[off] & 0x7f, mac[off + 1], mac[off + 2], mac[off + 3]]);
        let code = bin % 10u32.pow(self.digits);
        format!("{code:0width$}", width = self.digits as usize)
    }

    pub fn code_at_full(&self, t: u64) -> TotpCode {
        TotpCode { code: self.code_at(t), remaining: self.period - t % self.period, period: self.period, digits: self.digits }
    }

    pub fn now(&self) -> TotpCode {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.code_at_full(t)
    }
}

fn hmac_of<M: Mac + hmac::digest::KeyInit>(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut m = <M as hmac::digest::KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    m.update(msg);
    m.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const S1: &[u8] = b"12345678901234567890";
    const S256: &[u8] = b"12345678901234567890123456789012";
    const S512: &[u8] = b"1234567890123456789012345678901234567890123456789012345678901234";

    #[test]
    fn rfc6238_vectors() {
        let table: [(u64, &str, &str, &str); 6] = [
            (59, "94287082", "46119246", "90693936"),
            (1111111109, "07081804", "68084774", "25091201"),
            (1111111111, "14050471", "67062674", "99943326"),
            (1234567890, "89005924", "91819424", "93441116"),
            (2000000000, "69279037", "90698825", "38618901"),
            (20000000000, "65353130", "77737706", "47863826"),
        ];
        let t1 = Totp::new(S1.to_vec(), Algorithm::Sha1, 8, 30).unwrap();
        let t256 = Totp::new(S256.to_vec(), Algorithm::Sha256, 8, 30).unwrap();
        let t512 = Totp::new(S512.to_vec(), Algorithm::Sha512, 8, 30).unwrap();
        for (t, a, b, c) in table {
            assert_eq!(t1.code_at(t), a, "sha1 @ {t}");
            assert_eq!(t256.code_at(t), b, "sha256 @ {t}");
            assert_eq!(t512.code_at(t), c, "sha512 @ {t}");
        }
        let t6 = Totp::new(S1.to_vec(), Algorithm::Sha1, 6, 30).unwrap();
        assert_eq!(t6.code_at(59), "287082");
        let t60 = Totp::new(S1.to_vec(), Algorithm::Sha1, 8, 60).unwrap();
        assert_eq!(t60.code_at(118), t1.code_at(59));
        let c = t60.code_at_full(61);
        assert_eq!((c.remaining, c.period), (59, 60));
        assert_eq!(t1.code_at_full(30).remaining, 30);
    }

    #[test]
    fn base32_roundtrip() {
        assert_eq!(base32_encode(S1), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        assert_eq!(base32_decode("gezd gnbv-gy3tqojqgezdgnbvgy3tqojq====").unwrap(), S1);
        for n in 0..20 {
            let v: Vec<u8> = (0..n).map(|i: u8| i.wrapping_mul(37).wrapping_add(1)).collect();
            if n > 0 {
                assert_eq!(base32_decode(&base32_encode(&v)).unwrap(), v);
            }
        }
        assert!(base32_decode("ABC1").is_none());
        assert!(base32_decode("").is_none());
    }

    #[test]
    fn otpauth_uris() {
        let t = Totp::parse(
            "otpauth://totp/ACME%20Co:john.doe@email.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=ACME%20Co&algorithm=SHA256&digits=8&period=60",
        )
        .unwrap();
        assert_eq!(t.issuer.as_deref(), Some("ACME Co"));
        assert_eq!(t.account.as_deref(), Some("john.doe@email.com"));
        assert_eq!((t.algorithm, t.digits, t.period), (Algorithm::Sha256, 8, 60));
        let again = Totp::parse(&t.to_uri()).unwrap();
        assert_eq!(again.code_at(1_700_000_000), t.code_at(1_700_000_000));
        assert_eq!(again.info(), t.info());

        let plain = Totp::parse("  JBSWY3DPEHPK3PXP ").unwrap();
        assert_eq!((plain.algorithm, plain.digits, plain.period), (Algorithm::Sha1, 6, 30));
        let d = Totp::parse("OTPAUTH://TOTP/GitHub:me?secret=JBSWY3DPEHPK3PXP").unwrap();
        assert_eq!((d.issuer.as_deref(), d.digits, d.period), (Some("GitHub"), 6, 30));

        assert_eq!(Totp::parse("otpauth://hotp/x?secret=JBSWY3DPEHPK3PXP&counter=1").unwrap_err(), TotpError::Unsupported);
        assert_eq!(Totp::parse("otpauth-migration://offline?data=abc").unwrap_err(), TotpError::Unsupported);
        assert_eq!(Totp::parse("otpauth://totp/x?issuer=a").unwrap_err(), TotpError::Secret);
        assert_eq!(Totp::parse("otpauth://totp/x?secret=JBSWY3DP&digits=9").unwrap_err(), TotpError::Params);
        assert_eq!(Totp::parse("otpauth://totp/x?secret=JBSWY3DP&algorithm=MD5").unwrap_err(), TotpError::Params);
        assert_eq!(Totp::parse("https://example.com").unwrap_err(), TotpError::Invalid);
        assert_eq!(Totp::parse("not base32!").unwrap_err(), TotpError::Secret);
        assert!(!format!("{t:?}").contains("HXDM"), "Debug never prints the secret");
    }
}
