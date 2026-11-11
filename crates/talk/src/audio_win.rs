//! O microfone no Windows, pelo WASAPI (via `cpal`): o mesmo `Capture` do
//! `audio.rs`, que no Linux usa o `pw-record`.
//!
//! O WASAPI entrega na taxa e nos canais do mixer (quase sempre 48 kHz,
//! estéreo); aqui vira mono, e no fim 16 kHz.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use talk_core::resample::resample;

pub const RATE: usize = 16_000;

pub struct Capture {
    stream: Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    pub level: Arc<AtomicU32>,
    pub started: Instant,
}

/// Mono e medidor: a média dos canais de cada quadro vai para o buffer, e o
/// RMS do bloco para o medidor.
fn push(frames: impl Iterator<Item = f32>, samples: &Mutex<Vec<f32>>, level: &AtomicU32) {
    let mut out = samples.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let start = out.len();
    out.extend(frames.map(|s| if s.is_finite() { s } else { 0.0 }));
    let block = &out[start..];
    let sum: f32 = block.iter().map(|v| v * v).sum();
    let rms = (sum / block.len().max(1) as f32).sqrt();
    level.store(rms.to_bits(), Ordering::Relaxed);
}

fn device(target: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    match target {
        None => host.default_input_device().context("nenhum microfone no Windows"),
        Some(name) => host
            .input_devices()?
            .find(|d| d.description().is_ok_and(|x| x.name() == name))
            .with_context(|| format!("microfone {name:?} não encontrado (input_device no config)")),
    }
}

impl Capture {
    /// `target`: o nome do microfone como o Windows mostra; `None` = o padrão.
    pub fn start(target: Option<&str>) -> Result<Self> {
        let device = device(target)?;
        let supported = device.default_input_config().context("o microfone não informou o formato")?;
        let channels = usize::from(supported.channels()).max(1);
        let rate = supported.sample_rate();
        let config = supported.config();
        let samples = Arc::new(Mutex::new(Vec::with_capacity(rate as usize * 30)));
        let level = Arc::new(AtomicU32::new(0));
        let on_error = |e| eprintln!("[talk] microfone: {e}");

        let (buf, meter) = (samples.clone(), level.clone());
        let stream = match supported.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], _: &_| {
                    let mono = data.chunks(channels).map(|f| f.iter().sum::<f32>() / f.len() as f32);
                    push(mono, &buf, &meter);
                },
                on_error,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                config,
                move |data: &[i16], _: &_| {
                    let mono = data
                        .chunks(channels)
                        .map(|f| f.iter().map(|s| f32::from(*s) / 32_768.0).sum::<f32>() / f.len() as f32);
                    push(mono, &buf, &meter);
                },
                on_error,
                None,
            ),
            other => bail!("formato de microfone não suportado: {other:?}"),
        }
        .context("não deu para abrir o microfone")?;
        stream.play().context("o microfone não começou a gravar")?;
        Ok(Self { stream, samples, rate, level, started: Instant::now() })
    }

    pub fn stop(self) -> Result<Vec<f32>> {
        drop(self.stream); // fecha o fluxo: nenhum bloco chega depois disto
        let raw = Arc::try_unwrap(self.samples)
            .map_err(|_| anyhow!("o microfone ainda está gravando"))?
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(resample(&raw, self.rate, RATE as u32))
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}
