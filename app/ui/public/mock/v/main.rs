//! Conta le parole di un file.
use std::collections::HashMap;

/// Le parole più frequenti, in ordine.
fn top_words(text: &str, n: usize) -> Vec<(&str, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for w in text.split_whitespace() {
        *counts.entry(w).or_insert(0) += 1;
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.truncate(n);
    v
}

fn main() {
    let text = "il gatto e il cane e il topo";
    for (w, c) in top_words(text, 3) {
        println!("{w}: {c}"); // stampa
    }
    let _ok = true && 0x1F > 10;
}
