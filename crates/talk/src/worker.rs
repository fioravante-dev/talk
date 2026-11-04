//! O trabalhador: o único processo que toca na CUDA.
//!
//! O serviço sobe um trabalhador no instante em que o atalho é apertado, para
//! o modelo carregar enquanto a pessoa fala, e o manda embora depois de ocioso
//! (fechando a entrada dele). Terminar o processo é o que devolve toda a VRAM;
//! só liberar o modelo deixaria a CUDA segurando centenas de MB.

use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use talk_core::clean::{is_degenerate, is_known_hallucination, keep_speech};
use talk_core::wire::{self, Done, Job};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext,
    WhisperVadContextParams, WhisperVadParams,
    get_lang_str,
};

/// O lado de dentro: `talk-rs worker --model ...`.
pub fn run(model: &Path) -> Result<()> {
    let mut params = WhisperContextParameters::default();
    params.use_gpu(true).flash_attn(true);
    let path = model.to_str().context("caminho do modelo não é UTF-8")?;
    let ctx = WhisperContext::new_with_params(path, params)
        .with_context(|| format!("não deu para carregar {}", model.display()))?;
    let mut state = ctx.create_state()?;
    // O Silero do whisper.cpp. Sem ele, silêncio vira "Inscreva-se no canal e
    // ative o sininho": o Whisper preenche o vazio com o que mais viu no treino.
    // O whisper.cpp só roda o VAD no `whisper_full`, não no `full_with_state`
    // que o whisper-rs usa: o corte é feito aqui, antes de transcrever.
    let vad = talk_core::paths::models_dir().join(VAD_MODEL);
    let vad = vad.exists().then(|| vad.to_string_lossy().into_owned());
    if vad.is_none() {
        eprintln!("[worker] sem {VAD_MODEL}: transcrevendo sem detector de voz");
    }

    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = BufWriter::new(std::io::stdout().lock());
    while let Some((job, audio)) = wire::read_job(&mut input)? {
        let t = Instant::now();
        let done = match transcribe(&mut state, vad.as_deref(), &job, &audio) {
            Ok((text, language)) => {
                Done { text, language, stt_seconds: t.elapsed().as_secs_f64(), error: None }
            }
            Err(e) => Done {
                text: String::new(),
                language: String::new(),
                stt_seconds: t.elapsed().as_secs_f64(),
                error: Some(format!("{e:#}")),
            },
        };
        wire::send_done(&mut output, &done)?;
    }
    Ok(())
}

pub const VAD_MODEL: &str = "ggml-silero-v5.1.2.bin";

fn transcribe(
    state: &mut whisper_rs::WhisperState,
    vad: Option<&str>,
    job: &Job,
    audio: &[f32],
) -> Result<(String, String)> {
    // Menos de 0,3 s não é uma frase; o Whisper inventaria uma.
    if audio.len() < crate::audio::RATE * 3 / 10 {
        return Ok((String::new(), String::new()));
    }
    let mut full = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: job.beam_size.max(1),
        patience: -1.0,
    });
    full.set_language(Some(if job.language.is_empty() { "auto" } else { &job.language }));
    if !job.prompt.is_empty() {
        full.set_initial_prompt(&job.prompt);
    }
    // Ditado são falas curtas e independentes: arrastar contexto entre
    // segmentos é a principal fonte de alucinação em laço.
    full.set_no_context(true);
    full.set_no_timestamps(true);
    full.set_suppress_blank(true);
    full.set_print_progress(false);
    full.set_print_realtime(false);
    full.set_print_special(false);
    let speech;
    let audio = match vad {
        Some(path) => {
            // Um contexto novo por ditada: reaproveitado, ele fica mais lento a
            // cada chamada (17 → 34 ms com o mesmo áudio). Criar custa ~1 ms.
            // E uma thread só: as threads do ggml esperam girando, e com a CPU
            // ocupada (uma compilação, por exemplo) 16 ms viraram 5,7 s.
            let mut cp = WhisperVadContextParams::new();
            cp.set_n_threads(1);
            let mut vad = WhisperVadContext::new(path, cp)?;
            let mut vp = WhisperVadParams::new();
            vp.set_min_silence_duration(500); // o mesmo corte do Python
            let spans: Vec<(f32, f32)> =
                vad.segments_from_samples(vp, audio)?.map(|s| (s.start, s.end)).collect();
            speech = keep_speech(audio, &spans, crate::audio::RATE / 5);
            if speech.is_empty() {
                return Ok((String::new(), String::new()));
            }
            &speech[..]
        }
        None => audio,
    };
    state.full(full, audio)?;

    let mut parts = vec![];
    for seg in state.as_iter() {
        // O mesmo corte do Python: o Whisper se autoavalia como "sem fala".
        if seg.no_speech_probability() >= 0.7 {
            continue;
        }
        parts.push(seg.to_str_lossy()?.trim().to_owned());
    }
    let text = parts.join(" ").trim().to_owned();
    let language = get_lang_str(state.full_lang_id_from_state()).unwrap_or("").to_owned();
    let junk = is_degenerate(&text) || is_known_hallucination(&text);
    Ok((if junk { String::new() } else { text }, language))
}

/// O lado de fora: o processo filho visto pelo serviço.
pub struct Worker {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    pub model: PathBuf,
    pub last_used: Instant,
}

impl Worker {
    pub fn spawn(model: &Path, log: &Path) -> Result<Self> {
        let exe = std::env::current_exe()?;
        let stderr = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
        let mut child = Command::new(exe)
            .arg("worker")
            .arg("--model")
            .arg(model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .context("não deu para subir o trabalhador")?;
        let stdin = BufWriter::new(child.stdin.take().ok_or_else(|| anyhow!("sem stdin"))?);
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| anyhow!("sem stdout"))?);
        Ok(Self { child, stdin, stdout, model: model.to_owned(), last_used: Instant::now() })
    }

    pub fn transcribe(&mut self, job: &Job, audio: &[f32]) -> Result<Done> {
        wire::send_job(&mut self.stdin, job, audio)?;
        let done = wire::read_done(&mut self.stdout)?;
        self.last_used = Instant::now();
        Ok(done)
    }

    pub fn idle_for(&self) -> Duration {
        self.last_used.elapsed()
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Worker {
    /// Ocioso, ele só espera na entrada: matar não perde nada, e o `wait` evita
    /// zumbi. É aqui que a VRAM volta a zero.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
