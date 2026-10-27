use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const EN_WORDS: &str = include_str!("wordlists/en_eff_large.txt");
pub const IT_WORDS: &str = include_str!("wordlists/it.txt");

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!#$%&()*+,-./:;<=>?@[]^_{|}~";
pub const AMBIGUOUS: &str = "0O1lI|";

pub const MIN_LENGTH: usize = 4;
pub const MAX_LENGTH: usize = 128;
pub const MIN_WORDS: usize = 3;
pub const MAX_WORDS: usize = 20;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GenOptions {
    Random {
        #[serde(default = "d_len")]
        length: usize,
        #[serde(default = "yes")]
        lower: bool,
        #[serde(default = "yes")]
        upper: bool,
        #[serde(default = "yes")]
        digits: bool,
        #[serde(default = "yes")]
        symbols: bool,
        #[serde(default)]
        exclude_ambiguous: bool,
    },
    Passphrase {
        #[serde(default = "d_words")]
        words: usize,
        #[serde(default = "d_lang")]
        language: String,
        #[serde(default = "d_sep")]
        separator: String,
        #[serde(default)]
        capitalize: bool,
        #[serde(default)]
        number: bool,
    },
}

fn d_len() -> usize {
    20
}
fn yes() -> bool {
    true
}
fn d_words() -> usize {
    5
}
fn d_lang() -> String {
    "en".into()
}
fn d_sep() -> String {
    "-".into()
}

impl Default for GenOptions {
    fn default() -> Self {
        GenOptions::Random { length: 20, lower: true, upper: true, digits: true, symbols: true, exclude_ambiguous: false }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GenError {
    #[error("length must be {MIN_LENGTH}..={MAX_LENGTH}")]
    Length,
    #[error("choose at least one character class")]
    NoClass,
    #[error("words must be {MIN_WORDS}..={MAX_WORDS}")]
    Words,
    #[error("unknown wordlist language")]
    Language,
    #[error("separator is longer than 3 characters")]
    Separator,
}

pub struct Generated {
    pub value: Zeroizing<String>,
    pub entropy_bits: f64,
}

pub fn below(n: usize) -> usize {
    assert!(n > 0 && n <= u32::MAX as usize);
    let n = n as u32;
    let zone = u32::MAX - (u32::MAX % n);
    loop {
        let x = OsRng.next_u32();
        if x < zone {
            return (x % n) as usize;
        }
    }
}

pub fn wordlist(language: &str) -> Option<Vec<&'static str>> {
    match language {
        "en" => Some(EN_WORDS.lines().collect()),
        "it" => Some(IT_WORDS.lines().collect()),
        _ => None,
    }
}

pub fn generate(o: &GenOptions) -> Result<Generated, GenError> {
    match o {
        GenOptions::Random { length, lower, upper, digits, symbols, exclude_ambiguous } => {
            if !(MIN_LENGTH..=MAX_LENGTH).contains(length) {
                return Err(GenError::Length);
            }
            let classes: Vec<Vec<char>> = [(*lower, LOWER), (*upper, UPPER), (*digits, DIGITS), (*symbols, SYMBOLS)]
                .into_iter()
                .filter(|(on, _)| *on)
                .map(|(_, set)| set.chars().filter(|c| !(*exclude_ambiguous && AMBIGUOUS.contains(*c))).collect())
                .collect();
            if classes.is_empty() {
                return Err(GenError::NoClass);
            }
            let pool: Vec<char> = classes.concat();
            loop {
                let mut out = Zeroizing::new(String::with_capacity(*length));
                for _ in 0..*length {
                    out.push(pool[below(pool.len())]);
                }
                if classes.iter().all(|c| out.chars().any(|x| c.contains(&x))) {
                    return Ok(Generated { value: out, entropy_bits: random_entropy(&classes, *length) });
                }
            }
        }
        GenOptions::Passphrase { words, language, separator, capitalize, number } => {
            if !(MIN_WORDS..=MAX_WORDS).contains(words) {
                return Err(GenError::Words);
            }
            if separator.chars().count() > 3 {
                return Err(GenError::Separator);
            }
            let list = wordlist(language).ok_or(GenError::Language)?;
            let mut picked: Vec<Zeroizing<String>> = (0..*words)
                .map(|_| {
                    let w = list[below(list.len())];
                    Zeroizing::new(if *capitalize { capitalized(w) } else { w.to_owned() })
                })
                .collect();
            let mut bits = *words as f64 * (list.len() as f64).log2();
            if *number {
                let i = below(*words);
                picked[i].push(char::from(b'0' + below(10) as u8));
                bits += 10f64.log2() + (*words as f64).log2();
            }
            let mut out = Zeroizing::new(String::new());
            for (i, w) in picked.iter().enumerate() {
                if i > 0 {
                    out.push_str(separator);
                }
                out.push_str(w);
            }
            Ok(Generated { value: out, entropy_bits: bits })
        }
    }
}

fn capitalized(w: &str) -> String {
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn random_entropy(classes: &[Vec<char>], n: usize) -> f64 {
    let k = classes.len();
    let mut total = 0f64;
    for mask in 0u32..(1 << k) {
        let size: usize = (0..k).filter(|i| mask & (1 << i) == 0).map(|i| classes[i].len()).sum();
        let sign = if mask.count_ones() % 2 == 0 { 1.0 } else { -1.0 };
        total += sign * (size as f64).powi(n as i32);
    }
    total.log2()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random(length: usize, lower: bool, upper: bool, digits: bool, symbols: bool, exclude_ambiguous: bool) -> GenOptions {
        GenOptions::Random { length, lower, upper, digits, symbols, exclude_ambiguous }
    }

    #[test]
    fn wordlists_are_full_and_unique() {
        for lang in ["en", "it"] {
            let l = wordlist(lang).unwrap();
            assert_eq!(l.len(), 7776, "{lang}");
            let set: std::collections::HashSet<_> = l.iter().collect();
            assert_eq!(set.len(), 7776, "{lang} has duplicates");
            assert!(l.iter().all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c == '-')), "{lang}");
        }
    }

    #[test]
    fn random_respects_options() {
        for _ in 0..200 {
            let g = generate(&random(12, true, true, true, true, true)).unwrap();
            assert_eq!(g.value.chars().count(), 12);
            assert!(g.value.chars().any(|c| c.is_ascii_lowercase()));
            assert!(g.value.chars().any(|c| c.is_ascii_uppercase()));
            assert!(g.value.chars().any(|c| c.is_ascii_digit()));
            assert!(g.value.chars().any(|c| SYMBOLS.contains(c)));
            assert!(!g.value.chars().any(|c| AMBIGUOUS.contains(c)));
        }
        let d = generate(&random(30, false, false, true, false, false)).unwrap();
        assert!(d.value.chars().all(|c| c.is_ascii_digit()));
        assert!((d.entropy_bits - 30.0 * 10f64.log2()).abs() < 1e-6);
        assert_eq!(generate(&random(3, true, false, false, false, false)).err(), Some(GenError::Length));
        assert_eq!(generate(&random(200, true, false, false, false, false)).err(), Some(GenError::Length));
        assert_eq!(generate(&random(10, false, false, false, false, false)).err(), Some(GenError::NoClass));
    }

    #[test]
    fn random_entropy_is_sane() {
        let all = generate(&random(20, true, true, true, true, false)).unwrap().entropy_bits;
        let pool = (26 + 26 + 10 + SYMBOLS.len()) as f64;
        assert!(all < 20.0 * pool.log2() && all > 20.0 * pool.log2() - 1.0, "{all}");
        let short = generate(&random(4, true, true, true, true, false)).unwrap().entropy_bits;
        assert!(short < 4.0 * pool.log2() - 1.0);
    }

    #[test]
    fn values_differ_and_are_roughly_uniform() {
        let a = generate(&GenOptions::default()).unwrap();
        let b = generate(&GenOptions::default()).unwrap();
        assert_ne!(*a.value, *b.value);
        let mut counts = [0u32; 10];
        for _ in 0..20_000 {
            counts[below(10)] += 1;
        }
        assert!(counts.iter().all(|&c| (1700..2300).contains(&c)), "{counts:?}");
    }

    #[test]
    fn passphrases() {
        let o = GenOptions::Passphrase { words: 6, language: "it".into(), separator: " ".into(), capitalize: true, number: true };
        let g = generate(&o).unwrap();
        let parts: Vec<&str> = g.value.split(' ').collect();
        assert_eq!(parts.len(), 6);
        assert!(parts.iter().all(|p| p.chars().next().unwrap().is_uppercase()));
        assert_eq!(g.value.chars().filter(|c| c.is_ascii_digit()).count(), 1);
        let base = 6.0 * 7776f64.log2();
        assert!((g.entropy_bits - (base + 10f64.log2() + 6f64.log2())).abs() < 1e-9);
        let en = generate(&GenOptions::Passphrase { words: 5, language: "en".into(), separator: "_".into(), capitalize: false, number: false }).unwrap();
        let list = wordlist("en").unwrap();
        assert!(en.value.split('_').all(|w| list.contains(&w)), "words come from the list");
        assert!((en.entropy_bits - 5.0 * 7776f64.log2()).abs() < 1e-9);
        let bad = |words, language: &str, sep: &str| {
            generate(&GenOptions::Passphrase { words, language: language.into(), separator: sep.into(), capitalize: false, number: false }).err()
        };
        assert_eq!(bad(2, "en", "-"), Some(GenError::Words));
        assert_eq!(bad(5, "xx", "-"), Some(GenError::Language));
        assert_eq!(bad(5, "en", "----"), Some(GenError::Separator));
    }

    #[test]
    fn options_from_json_use_defaults() {
        let o: GenOptions = serde_json::from_str(r#"{"kind":"passphrase"}"#).unwrap();
        assert_eq!(o, GenOptions::Passphrase { words: 5, language: "en".into(), separator: "-".into(), capitalize: false, number: false });
        let r: GenOptions = serde_json::from_str(r#"{"kind":"random","length":32,"symbols":false}"#).unwrap();
        assert_eq!(r, random(32, true, true, true, false, false));
    }
}
