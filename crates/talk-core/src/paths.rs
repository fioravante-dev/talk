//! Onde o talk guarda as coisas: os mesmos lugares da versão em Python, que já
//! saiu, para o config, o vocabulário e o histórico de antes continuarem valendo.

use std::env;
use std::path::PathBuf;

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(fallback))
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("talk")
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("talk")
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn vocab_file() -> PathBuf {
    config_dir().join("vocab.txt")
}

pub fn history_file() -> PathBuf {
    cache_dir().join("history.jsonl")
}

pub fn models_dir() -> PathBuf {
    cache_dir().join("models")
}

/// O socket do serviço.
pub fn socket() -> PathBuf {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    runtime.join("talk-rs.sock")
}
