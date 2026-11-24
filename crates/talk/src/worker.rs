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
use talk_core::wire::{self, Done, Job};
use whisper_rs::{WhisperContext, WhisperContextParameters};

use crate::stt::{VAD_MODEL, transcribe};

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

/// `CREATE_NO_WINDOW`: sem isto, um serviço sem console que abre um processo
/// de console faz uma janela preta piscar na tela a cada ditada.
#[cfg(windows)]
pub const NO_WINDOW: u32 = 0x0800_0000;
/// `CREATE_NEW_PROCESS_GROUP`: o Ctrl+C de quem chamou não chega no filho.
#[cfg(windows)]
pub const DETACHED: u32 = 0x0000_0200;

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
        let mut cmd = Command::new(exe);
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, NO_WINDOW);
        let mut child = cmd
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
