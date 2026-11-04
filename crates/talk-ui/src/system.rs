//! O que a janela lê do sistema: os microfones do PipeWire, a RAM do serviço e
//! a VRAM do trabalhador, e o estado pelo socket do serviço.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;
use talk_core::paths;

#[derive(Debug, Clone, Default)]
pub struct Status {
    /// idle, recording, transcribing ou off (serviço parado).
    pub phase: String,
    pub pid: Option<u32>,
    pub model_loaded: bool,
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
            pid: v["pid"].as_u64().and_then(|p| u32::try_from(p).ok()),
            model_loaded: v["model_loaded"].as_bool().unwrap_or(false),
        },
        None => Status { phase: "off".into(), ..Status::default() },
    }
}

pub fn rss_mb(pid: u32) -> Option<f64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kb: f64 = status.lines().find(|l| l.starts_with("VmRSS:"))?.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024.0)
}

fn is_worker(pid: &str) -> bool {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|c| {
            let args: Vec<&[u8]> = c.split(|b| *b == 0).collect();
            args.first().is_some_and(|a| a.ends_with(b"talk-rs")) && args.get(1) == Some(&&b"worker"[..])
        })
        .unwrap_or(false)
}

/// A VRAM dos trabalhadores do talk. `None` = sem nvidia-smi.
pub fn worker_vram_mb() -> Option<u64> {
    let out = Command::new("nvidia-smi")
        .args(["--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    Some(
        text.lines()
            .filter_map(|l| l.split_once(','))
            .filter(|(pid, _)| is_worker(pid.trim()))
            .filter_map(|(_, mib)| mib.trim().parse::<u64>().ok())
            .sum(),
    )
}

#[derive(Debug, Clone)]
pub struct Mic {
    pub name: String,
    pub description: String,
}

/// Os microfones e o padrão atual, pelo `pw-dump`.
pub fn mics() -> (Vec<Mic>, Option<String>) {
    let Ok(out) = Command::new("pw-dump").output() else { return (vec![], None) };
    let Ok(Value::Array(objs)) = serde_json::from_slice::<Value>(&out.stdout) else { return (vec![], None) };
    let mut mics = vec![];
    let mut default = None;
    for o in &objs {
        let props = &o["info"]["props"];
        if props["media.class"] == "Audio/Source"
            && let Some(name) = props["node.name"].as_str()
        {
            let description = props["node.description"].as_str().unwrap_or(name).to_owned();
            mics.push(Mic { name: name.to_owned(), description });
        }
        if o["type"] == "PipeWire:Interface:Metadata" && o["props"]["metadata.name"] == "default" {
            for m in o["metadata"].as_array().into_iter().flatten() {
                if m["key"] == "default.audio.source" {
                    default = m["value"]["name"].as_str().map(str::to_owned);
                }
            }
        }
    }
    (mics, default)
}

/// Copia para a área de transferência. Aqui não precisa esperar a cópia valer:
/// ninguém vai colar no mesmo instante.
pub fn copy(text: &str) -> anyhow::Result<()> {
    let mut child = Command::new("wl-copy").stdin(std::process::Stdio::piped()).spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    child.wait()?;
    Ok(())
}
