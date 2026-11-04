//! O vocabulário: a dica que vai para o Whisper e as trocas aplicadas depois.
//!
//! Formato do `vocab.txt`, uma linha por termo:
//!
//! ```text
//! Wayland                         só a dica
//! Wayland = Weylend, Uêilend      a dica, e essas grafias viram "Wayland"
//! # comentário
//! ```
//!
//! A ordem da dica importa: o Whisper (o whisper.cpp e o faster-whisper) guarda
//! só os ÚLTIMOS ~224 tokens do prompt e corta o começo. Por isso o jargão
//! genérico do `vocab` do config vai na frente e os termos do `vocab.txt`, que
//! são os pessoais, no fim.

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub term: String,
    pub errors: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Vocab {
    pub entries: Vec<Entry>,
    /// Termos do `vocab` do config, separados por vírgula.
    pub generic: Vec<String>,
    swaps: Vec<(Regex, String)>,
}

fn split_terms(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(',')
        .map(|t| t.trim().trim_end_matches('.').trim())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

pub fn parse_file(text: &str) -> Vec<Entry> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let (term, errors) = match line.split_once('=') {
                Some((t, e)) => (t.trim(), split_terms(e).collect()),
                None => (line, vec![]),
            };
            (!term.is_empty()).then(|| Entry { term: term.to_owned(), errors })
        })
        .collect()
}

/// Uma linha do arquivo para o termo.
pub fn format_entry(e: &Entry) -> String {
    if e.errors.is_empty() { e.term.clone() } else { format!("{} = {}", e.term, e.errors.join(", ")) }
}

fn is_entry_for(line: &str, term: &str) -> bool {
    let l = line.trim();
    if l.is_empty() || l.starts_with('#') {
        return false;
    }
    let t = l.split_once('=').map_or(l, |(t, _)| t).trim();
    t.eq_ignore_ascii_case(term)
}

/// Põe ou troca a linha do termo, sem mexer nos comentários nem nas outras.
pub fn upsert(file: &str, entry: &Entry) -> String {
    let line = format_entry(entry);
    let mut found = false;
    let mut out: Vec<String> = file
        .lines()
        .map(|l| {
            if !found && is_entry_for(l, &entry.term) {
                found = true;
                line.clone()
            } else {
                l.to_owned()
            }
        })
        .collect();
    if !found {
        out.push(line);
    }
    out.join("\n") + "\n"
}

pub fn remove(file: &str, term: &str) -> String {
    let out: Vec<&str> = file.lines().filter(|l| !is_entry_for(l, term)).collect();
    out.join("\n") + "\n"
}

/// O Whisper corta a dica em ~224 tokens. Sem o tokenizer aqui, uma estimativa
/// medida no tokenizer do large-v3: 564 caracteres deram 202 tokens.
pub const PROMPT_TOKEN_LIMIT: usize = 224;

pub fn estimate_tokens(prompt: &str) -> usize {
    (prompt.chars().count() as f64 / 2.8).ceil() as usize
}

impl Vocab {
    pub fn new(file: &str, config_vocab: &str) -> Self {
        let entries = parse_file(file);
        let generic = split_terms(config_vocab).collect();
        let swaps = entries
            .iter()
            .filter(|e| !e.errors.is_empty())
            .filter_map(|e| {
                // A mais longa primeiro: "Wey land" antes de "Wey".
                let mut errors: Vec<&str> = e.errors.iter().map(String::as_str).collect();
                errors.sort_by_key(|s| std::cmp::Reverse(s.len()));
                let alts: Vec<String> = errors.iter().map(|s| regex::escape(s)).collect();
                let re = Regex::new(&format!(r"(?i)\b(?:{})\b", alts.join("|"))).ok()?;
                Some((re, e.term.clone()))
            })
            .collect();
        Self { entries, generic, swaps }
    }

    /// A dica para o Whisper: o genérico na frente, o pessoal no fim, sem
    /// repetir termo (sem diferenciar maiúsculas).
    pub fn prompt(&self) -> String {
        let personal: Vec<&str> = self.entries.iter().map(|e| e.term.as_str()).collect();
        let is_personal = |t: &str| personal.iter().any(|p| p.eq_ignore_ascii_case(t));
        let mut out: Vec<&str> = vec![];
        for t in self.generic.iter().map(String::as_str).filter(|t| !is_personal(t)).chain(personal.iter().copied()) {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
                out.push(t);
            }
        }
        if out.is_empty() { String::new() } else { format!("{}.", out.join(", ")) }
    }

    /// Troca as grafias erradas conhecidas pelo termo. Só palavra inteira:
    /// "Weylend" vira "Wayland", "Weylendia" não.
    pub fn apply(&self, text: &str) -> String {
        self.swaps
            .iter()
            .fold(text.to_owned(), |acc, (re, term)| re.replace_all(&acc, term.as_str()).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
# Termos que o Whisper erra.
Ollama = olhama, olama, Olamma
Wayland = Weylend, Uêilend, Wey land
Tauri
";

    #[test]
    fn parses_terms_errors_and_skips_comments() {
        let entries = parse_file(FILE);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].term, "Wayland");
        assert_eq!(entries[1].errors, ["Weylend", "Uêilend", "Wey land"]);
        assert!(entries[2].errors.is_empty());
    }

    #[test]
    fn prompt_puts_the_personal_terms_last_because_whisper_keeps_the_tail() {
        let v = Vocab::new(FILE, "Claude, Rust, tauri, PR.");
        assert_eq!(v.prompt(), "Claude, Rust, PR, Ollama, Wayland, Tauri.");
    }

    #[test]
    fn empty_vocab_is_no_prompt() {
        assert_eq!(Vocab::new("", "").prompt(), "");
    }

    #[test]
    fn upsert_replaces_in_place_and_keeps_comments() {
        let e = Entry { term: "Wayland".into(), errors: vec!["Weylend".into()] };
        let out = upsert(FILE, &e);
        assert!(out.starts_with("# Termos que o Whisper erra.\nOllama"));
        assert!(out.contains("\nWayland = Weylend\nTauri\n"));
        let out = upsert(&out, &Entry { term: "T3 Code".into(), errors: vec![] });
        assert!(out.ends_with("Tauri\nT3 Code\n"));
        let out = remove(&out, "tauri");
        assert!(!out.contains("Tauri\n") && out.contains("T3 Code"));
    }

    #[test]
    fn estimates_like_the_measured_tokenizer() {
        assert_eq!(estimate_tokens(&"x".repeat(564)), 202);
    }

    #[test]
    fn swaps_whole_words_ignoring_case() {
        let v = Vocab::new(FILE, "");
        assert_eq!(v.apply("o weylend e o Wey land"), "o Wayland e o Wayland");
        assert_eq!(v.apply("roda no olhama"), "roda no Ollama");
        assert_eq!(v.apply("weylendia"), "weylendia");
    }
}
