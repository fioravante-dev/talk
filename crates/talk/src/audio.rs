//! O microfone, pelo `pw-record`: o mesmo caminho que o talk em Python já
//! provou no PipeWire, sem biblioteca de áudio para compilar.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use talk_core::clean::au_header_len;

pub const RATE: usize = 16_000;

pub struct Capture {
    child: Child,
    reader: JoinHandle<Vec<f32>>,
    /// O nível do microfone (RMS do último bloco, em f32 como bits), para o
    /// medidor da janela e da pílula.
    pub level: Arc<AtomicU32>,
    pub started: Instant,
}

impl Capture {
    /// `target`: o `node.name` do microfone; `None` = o padrão do sistema.
    pub fn start(target: Option<&str>) -> Result<Self> {
        let mut cmd = Command::new("pw-record");
        cmd.args(["--rate", "16000", "--channels", "1", "--format", "f32"]);
        if let Some(t) = target {
            cmd.args(["--target", t]);
        }
        let mut child = cmd
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("não deu para abrir o pw-record (vem com o PipeWire)")?;
        let mut out = child.stdout.take().ok_or_else(|| anyhow!("pw-record sem saída"))?;
        let level = Arc::new(AtomicU32::new(0));
        let meter = level.clone();
        let reader = std::thread::spawn(move || {
            let mut raw = Vec::with_capacity(RATE * 4 * 30);
            let mut buf = [0u8; 3200]; // 50 ms
            let mut skip: Option<usize> = None;
            loop {
                let n = match out.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                raw.extend_from_slice(&buf[..n]);
                if skip.is_none() {
                    skip = au_header_len(&raw);
                }
                let block = &buf[..n - n % 4];
                let sum: f32 = block
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| f32::from_le_bytes(*c).powi(2))
                    .filter(|v| v.is_finite())
                    .sum();
                let rms = (sum / (block.len() / 4).max(1) as f32).sqrt();
                meter.store(rms.to_bits(), Ordering::Relaxed);
            }
            let body = raw.get(skip.unwrap_or(0)..).unwrap_or_default();
            body.as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                // Um NaN contamina o espectrograma inteiro: vale mais zerar uma
                // amostra do que perder a frase.
                .map(|s| if s.is_finite() { s } else { 0.0 })
                .collect()
        });
        Ok(Self { child, reader, level, started: Instant::now() })
    }

    /// Para com SIGINT, para o pw-record descarregar o que tem; não SIGKILL.
    pub fn stop(mut self) -> Result<Vec<f32>> {
        let pid = i32::try_from(self.child.id()).context("pid")?;
        let _ = kill(Pid::from_raw(pid), Signal::SIGINT);
        let _ = self.child.wait();
        self.reader.join().map_err(|_| anyhow!("a leitura do microfone falhou"))
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}
