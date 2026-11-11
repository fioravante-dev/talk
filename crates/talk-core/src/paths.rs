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

/// No Windows: `%APPDATA%\talk` (o config acompanha o perfil) e
/// `%LOCALAPPDATA%\talk` (modelos e histórico ficam na máquina).
#[cfg(windows)]
fn windows_dir(var: &str) -> Option<PathBuf> {
    env::var_os(var).filter(|v| !v.is_empty()).map(|v| PathBuf::from(v).join("talk"))
}

pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(d) = windows_dir("APPDATA") {
        return d;
    }
    xdg("XDG_CONFIG_HOME", ".config").join("talk")
}

pub fn cache_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(d) = windows_dir("LOCALAPPDATA") {
        return d;
    }
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

/// O socket do serviço. O Windows 10+ também tem sockets Unix, num arquivo.
#[cfg(windows)]
pub fn socket() -> PathBuf {
    cache_dir().join("talk-rs.sock")
}

/// O socket do serviço.
#[cfg(not(windows))]
pub fn socket() -> PathBuf {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    runtime.join("talk-rs.sock")
}
