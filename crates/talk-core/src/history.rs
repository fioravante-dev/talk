//! A linha do `history.jsonl`. O formato é o da versão em Python, então as
//! ditadas de antes continuam no histórico.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct Line<'a> {
    pub at: String,
    pub text: &'a str,
    pub language: &'a str,
    pub motor: &'a str,
    pub delivered: &'a [&'a str],
    pub audio_seconds: f64,
    pub stt_seconds: f64,
}

/// Uma ditada lida de volta, escrita pelo Rust ou pelo Python.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Entry {
    pub at: String,
    pub text: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub motor: String,
    #[serde(default)]
    pub audio_seconds: f64,
    #[serde(default)]
    pub stt_seconds: f64,
}

/// As mais recentes primeiro. Uma linha que não se lê é pulada, não derruba
/// o histórico inteiro.
pub fn read(path: &Path, limit: usize) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(path) else { return vec![] };
    text.lines().rev().filter_map(|l| serde_json::from_str(l).ok()).take(limit).collect()
}

pub fn now() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Só entra o que chegou a algum lugar e tem texto, como no Python.
pub fn append(path: &Path, line: &Line) -> Result<bool> {
    if line.text.is_empty() || line.delivered.is_empty() {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(line)?)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line<'a>(text: &'a str, delivered: &'a [&'a str]) -> Line<'a> {
        Line {
            at: "2026-11-03T14:00:00".into(),
            text,
            language: "pt",
            motor: "whisper.cpp",
            delivered,
            audio_seconds: 2.0,
            stt_seconds: 0.4,
        }
    }

    #[test]
    fn appends_the_python_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        assert!(append(&path, &line("oi", &["paste"])).unwrap());
        let got: serde_json::Value =
            serde_json::from_str(std::fs::read_to_string(&path).unwrap().trim()).unwrap();
        assert_eq!(got["text"], "oi");
        assert_eq!(got["delivered"][0], "paste");
        assert_eq!(got["motor"], "whisper.cpp");
    }

    #[test]
    fn reads_back_newest_first_and_skips_broken_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        append(&path, &line("primeira", &["paste"])).unwrap();
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "{quebrada\n").unwrap();
        append(&path, &line("segunda", &["paste"])).unwrap();
        let got = read(&path, 10);
        assert_eq!(got.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["segunda", "primeira"]);
    }

    #[test]
    fn nothing_delivered_is_not_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        assert!(!append(&path, &line("oi", &[])).unwrap());
        assert!(!append(&path, &line("", &["paste"])).unwrap());
        assert!(!path.exists());
    }
}
