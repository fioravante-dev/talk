//! O alto-falante da fala no Windows, pelo WASAPI (via `cpal`): o mesmo
//! `Player`/`Sink` do `player.rs`, que no Linux usa o `pw-play`.
//!
//! O WASAPI toca na taxa e nos canais do mixer (quase sempre 48 kHz,
//! estéreo). As amostras entram na taxa da voz (24 kHz) e são convertidas na
//! hora, por interpolação linear, no próprio callback: as frases chegam em
//! pedaços, e assim não há emenda entre eles.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

#[derive(Default)]
struct Queue {
    samples: VecDeque<f32>,
    /// Entre `samples[0]` e `samples[1]`, de 0 a 1.
    pos: f64,
    /// O `Sink` foi largado: não chega mais nada.
    closed: bool,
    killed: bool,
}

type Shared = Arc<Mutex<Queue>>;

fn lock(q: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    q.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct Player {
    queue: Shared,
    done: Arc<AtomicBool>,
}

pub struct Sink {
    queue: Shared,
}

impl Queue {
    /// A próxima amostra na taxa do mixer; silêncio quando falta.
    fn next(&mut self, step: f64) -> f32 {
        if self.killed || self.samples.is_empty() {
            return 0.0;
        }
        let a = self.samples[0];
        let b = self.samples.get(1).copied().unwrap_or(a);
        let v = a + (b - a) * self.pos as f32;
        self.pos += step;
        while self.pos >= 1.0 && !self.samples.is_empty() {
            self.pos -= 1.0;
            self.samples.pop_front();
        }
        v
    }

    fn drained(&self) -> bool {
        self.killed || (self.closed && self.samples.is_empty())
    }
}

/// Abre o alto-falante padrão para áudio mono na taxa `rate`. O fluxo do
/// WASAPI não é `Send`: vive numa thread só dele, até o áudio acabar.
pub fn open(rate: u32) -> Result<(Player, Sink)> {
    let queue: Shared = Arc::default();
    let done = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
    let (q, finished) = (queue.clone(), done.clone());
    thread::spawn(move || {
        let stream = match build(rate, q.clone()) {
            Ok(s) => s,
            Err(e) => {
                finished.store(true, Ordering::SeqCst);
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        let _ = ready_tx.send(Ok(()));
        while !lock(&q).drained() {
            thread::sleep(Duration::from_millis(20));
        }
        // O que já foi para o mixer ainda está tocando: espera o buffer
        // dele esvaziar antes de fechar, senão a última sílaba some.
        if !lock(&q).killed {
            thread::sleep(Duration::from_millis(150));
        }
        drop(stream);
        finished.store(true, Ordering::SeqCst);
    });
    ready_rx.recv().map_err(|_| anyhow!("a thread do alto-falante morreu"))??;
    Ok((Player { queue: queue.clone(), done }, Sink { queue }))
}

fn build(rate: u32, queue: Shared) -> Result<cpal::Stream> {
    let device = cpal::default_host().default_output_device().context("nenhum alto-falante no Windows")?;
    let supported = device.default_output_config().context("o alto-falante não informou o formato")?;
    let channels = usize::from(supported.channels()).max(1);
    let step = f64::from(rate) / f64::from(supported.sample_rate());
    let config = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => output::<f32>(&device, &config, channels, step, queue),
        SampleFormat::I16 => output::<i16>(&device, &config, channels, step, queue),
        other => bail!("formato de alto-falante não suportado: {other:?}"),
    }?;
    stream.play().context("o alto-falante não começou a tocar")?;
    Ok(stream)
}

fn output<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    step: f64,
    queue: Shared,
) -> Result<cpal::Stream> {
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &_| {
                let mut q = lock(&queue);
                for frame in data.chunks_mut(channels) {
                    let v = T::from_sample(q.next(step));
                    frame.fill(v);
                }
            },
            |e| eprintln!("[speech] alto-falante: {e}"),
            None,
        )
        .context("não deu para abrir o alto-falante")
}

impl Sink {
    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        let mut q = lock(&self.queue);
        if q.killed {
            bail!("cortado");
        }
        q.samples.extend(samples.iter().map(|s| if s.is_finite() { *s } else { 0.0 }));
        Ok(())
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        lock(&self.queue).closed = true;
    }
}

impl Player {
    pub fn kill(&mut self) {
        lock(&self.queue).killed = true;
        while !self.finished() {
            thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn finished(&mut self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}
