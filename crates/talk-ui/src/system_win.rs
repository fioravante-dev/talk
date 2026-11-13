//! O que a janela lê do sistema, no Windows: o mesmo contrato do `system.rs`.
//!
//! - microfones pelo WASAPI (`cpal`), com os nomes que o serviço procura;
//! - RAM do serviço: ele mesmo informa no `status` (perguntar ao `tasklist`
//!   custava ~100 ms a cada página);
//! - VRAM: no Windows (WDDM) o `nvidia-smi` não diz quanto cada processo usa,
//!   então volta `None` e a janela mostra só se o modelo está carregado.

use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait};
use serde_json::Value;
use talk_core::paths;
use uds_windows::UnixStream;

#[derive(Debug, Clone, Default)]
pub struct Status {
    /// idle, recording, transcribing ou off (serviço parado).
    pub phase: String,
    pub model_loaded: bool,
    pub rss_mb: Option<f64>,
}

pub fn status() -> Status {
    let ask = || -> Option<Value> {
        let s = UnixStream::connect(paths::socket()).ok()?;
        s.set_read_timeout(Some(Duration::from_millis(400))).ok()?;
        let mut out = &s;
        writeln!(out, r#"{{"cmd":"status"}}"#).ok()?;
        let mut line = String::new();
        BufReader::new(&s).read_line(&mut line).ok()?;
        serde_json::from_str(&line).ok()
    };
    match ask() {
        Some(v) => Status {
            phase: v["phase"].as_str().unwrap_or("idle").to_owned(),
            model_loaded: v["model_loaded"].as_bool().unwrap_or(false),
            rss_mb: v["rss_mb"].as_f64(),
        },
        None => Status { phase: "off".into(), ..Status::default() },
    }
}

pub fn service_mb(st: &Status) -> Option<f64> {
    st.rss_mb
}

pub fn worker_vram_mb() -> Option<u64> {
    None
}

#[derive(Debug, Clone)]
pub struct Mic {
    pub name: String,
    pub description: String,
}

/// Os microfones e o padrão atual. O nome é o mesmo que o serviço compara com
/// o `input_device` do config.
pub fn mics() -> (Vec<Mic>, Option<String>) {
    let host = cpal::default_host();
    let name = |d: &cpal::Device| d.description().ok().map(|x| x.name().to_owned());
    let default = host.default_input_device().as_ref().and_then(name);
    let mics = host
        .input_devices()
        .map(|ds| ds.filter_map(|d| name(&d)).map(|n| Mic { description: n.clone(), name: n }).collect())
        .unwrap_or_default();
    (mics, default)
}

pub fn copy(text: &str) -> anyhow::Result<()> {
    arboard::Clipboard::new()?.set_text(text)?;
    Ok(())
}
