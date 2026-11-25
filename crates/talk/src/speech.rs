//! A fala: `talk-rs speak "texto"` → prosa → frases → Qwen3-TTS → `pw-play`.
//!
//! O mesmo desenho do ditado ao contrário. O serviço nunca toca na CUDA: quem
//! toca é um trabalhador em Python (o `faster-qwen3-tts` só existe lá), que sobe
//! na primeira fala e sai depois de ocioso (`speech_keep_alive`), devolvendo a
//! VRAM. As falas entram numa fila e saem uma depois da outra; `hush` (ou
//! começar uma ditada) corta a que está tocando e esvazia a fila.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use talk_core::config::Config;
use talk_core::paths;
use talk_core::speech::{self, Say};

/// O script do trabalhador vai dentro do binário: atualizar o talk atualiza os dois.
const WORKER_PY: &str = include_str!("speech_worker.py");
/// Frases mais longas que isto são cortadas: o primeiro som sai antes.
const MAX_SENTENCE: usize = 220;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn speech_dir() -> PathBuf {
    paths::cache_dir().join("speech")
}

fn python(cfg: &Config) -> PathBuf {
    if cfg.speech_python.is_empty() {
        speech_dir().join("venv/bin/python")
    } else {
        PathBuf::from(&cfg.speech_python)
    }
}

/// O processo Python, visto daqui.
struct Worker {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    model: String,
    last_used: Instant,
}

impl Worker {
    fn spawn(cfg: &Config) -> Result<Self> {
        let dir = speech_dir();
        std::fs::create_dir_all(&dir)?;
        let script = dir.join("speech_worker.py");
        if std::fs::read_to_string(&script).ok().as_deref() != Some(WORKER_PY) {
            std::fs::write(&script, WORKER_PY)?;
        }
        let py = python(cfg);
        if !py.exists() {
            anyhow::bail!("sem o ambiente da fala ({}): rode o scripts/install.sh", py.display());
        }
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("speech.log"))?;
        let mut child = Command::new(&py)
            .arg(&script)
            .arg(&cfg.speech_model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log)
            .spawn()
            .with_context(|| format!("não deu para subir {}", py.display()))?;
        let stdin = BufWriter::new(child.stdin.take().ok_or_else(|| anyhow!("sem stdin"))?);
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| anyhow!("sem stdout"))?);
        eprintln!("[speech] trabalhador subindo ({})", cfg.speech_model);
        Ok(Self { child, stdin, stdout, model: cfg.speech_model.clone(), last_used: Instant::now() })
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Uma fala na fila, com a "geração" de quando entrou: um `hush` sobe a
/// geração, e tudo o que entrou antes dele é descartado.
struct Utterance {
    text: String,
    generation: u64,
}

#[derive(Clone)]
pub struct Speech {
    queue: mpsc::Sender<Utterance>,
    generation: Arc<AtomicU64>,
    speaking: Arc<AtomicBool>,
    /// O `pw-play` tocando agora: o `hush` o mata e o som para na hora.
    player: Arc<Mutex<Option<Child>>>,
    worker: Arc<Mutex<Option<Worker>>>,
}

impl Speech {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<Utterance>();
        let me = Self {
            queue: tx,
            generation: Arc::new(AtomicU64::new(0)),
            speaking: Arc::new(AtomicBool::new(false)),
            player: Arc::new(Mutex::new(None)),
            worker: Arc::new(Mutex::new(None)),
        };
        let speaker = me.clone();
        thread::spawn(move || {
            for u in rx {
                if speaker.cancelled(u.generation) {
                    continue;
                }
                speaker.speaking.store(true, Ordering::SeqCst);
                if let Err(e) = speaker.say(&u.text, u.generation) {
                    eprintln!("[speech] {e:#}");
                }
                speaker.speaking.store(false, Ordering::SeqCst);
            }
        });
        me.reap();
        me
    }

    /// Põe na fila e volta logo: quem chama (o Claude) não espera a fala acabar.
    pub fn enqueue(&self, text: String) {
        let generation = self.generation.load(Ordering::SeqCst);
        let _ = self.queue.send(Utterance { text, generation });
    }

    /// Cala: corta o som agora e descarta a fila.
    pub fn hush(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut p) = lock(&self.player).take() {
            let _ = p.kill();
            let _ = p.wait();
        }
    }

    pub fn speaking(&self) -> bool {
        self.speaking.load(Ordering::SeqCst)
    }

    /// Ocupado falando conta como carregado: o `status` não espera a frase acabar.
    pub fn loaded(&self) -> bool {
        match self.worker.try_lock() {
            Ok(mut w) => w.as_mut().is_some_and(Worker::alive),
            Err(_) => true,
        }
    }

    fn cancelled(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) != generation
    }

    fn say(&self, text: &str, generation: u64) -> Result<()> {
        let cfg = Config::load(&paths::config_file()).unwrap_or_default();
        let prose = speech::to_speech(text);
        if prose.is_empty() {
            return Ok(());
        }
        let language = speech::detect_language(&prose);
        let speaker = cfg.speech_voice(language).to_owned();
        let started = Instant::now();
        let mut first_sound: Option<Duration> = None;
        let mut out: Option<ChildStdin> = None;

        for sentence in speech::sentences(&prose, MAX_SENTENCE) {
            if self.cancelled(generation) {
                break;
            }
            let mut guard = lock(&self.worker);
            if !guard.as_mut().is_some_and(|w| w.alive() && w.model == cfg.speech_model) {
                *guard = None; // o antigo sai antes, para os dois não disputarem a VRAM
                *guard = Some(Worker::spawn(&cfg)?);
            }
            let w = guard.as_mut().ok_or_else(|| anyhow!("sem trabalhador da fala"))?;
            let say = Say { text: sentence, language: language.into(), speaker: speaker.clone() };
            if let Err(e) = speech::send_say(&mut w.stdin, &say) {
                *guard = None;
                return Err(e).context("o trabalhador da fala caiu (veja ~/.cache/talk/speech/speech.log)");
            }
            loop {
                let (chunk, audio) = match speech::read_chunk(&mut w.stdout) {
                    Ok(c) => c,
                    Err(e) => {
                        *guard = None;
                        return Err(e).context("o trabalhador da fala caiu (veja ~/.cache/talk/speech/speech.log)");
                    }
                };
                if chunk.done {
                    if let Some(e) = chunk.error {
                        eprintln!("[speech] frase falhou: {e}");
                    }
                    break;
                }
                // Cancelado no meio: o resto da frase ainda chega e é jogado fora,
                // para o fio seguir alinhado para a próxima.
                if self.cancelled(generation) || audio.is_empty() {
                    continue;
                }
                if out.is_none() {
                    out = Some(self.play(chunk.rate)?);
                    first_sound.get_or_insert(started.elapsed());
                }
                let bytes: Vec<u8> = audio.iter().flat_map(|s| s.to_le_bytes()).collect();
                if out.as_mut().is_some_and(|o| o.write_all(&bytes).is_err()) {
                    out = None; // o pw-play morreu (hush): o resto é descartado
                }
            }
            w.last_used = Instant::now();
        }

        // Fecha a entrada do pw-play e espera ele tocar o que falta, sem segurar
        // o lock: um `hush` no meio ainda o mata.
        drop(out);
        loop {
            let mut p = lock(&self.player);
            match p.as_mut().map(Child::try_wait) {
                Some(Ok(None)) => {}
                _ => {
                    *p = None;
                    break;
                }
            }
            drop(p);
            thread::sleep(Duration::from_millis(50));
        }
        if let Some(t) = first_sound {
            eprintln!(
                "[speech] {} ({language}, {speaker}): 1º som {:.2}s, total {:.1}s",
                hash(&prose),
                t.as_secs_f64(),
                started.elapsed().as_secs_f64()
            );
        }
        Ok(())
    }

    /// Um `pw-play` cru, mono, f32, na taxa do modelo. Volta a entrada dele.
    fn play(&self, rate: u32) -> Result<ChildStdin> {
        let mut child = Command::new("pw-play")
            .args(["--raw", "--format", "f32", "--channels", "1", "--rate", &rate.to_string()])
            .args(["--latency", "50ms", "--media-role", "Communication", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("não deu para abrir o pw-play (vem com o PipeWire)")?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("pw-play sem entrada"))?;
        *lock(&self.player) = Some(child);
        Ok(stdin)
    }

    /// Manda o trabalhador embora depois de ocioso: é aqui que a VRAM volta.
    fn reap(&self) {
        let me = self.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(5));
                if me.speaking() {
                    continue;
                }
                let Ok(Some(ttl)) = Config::load(&paths::config_file()).and_then(|c| c.speech_keep_alive()) else {
                    continue;
                };
                let mut w = lock(&me.worker);
                if w.as_ref().is_some_and(|w| w.last_used.elapsed() >= ttl) {
                    eprintln!("[speech] trabalhador ocioso, saindo");
                    *w = None;
                }
            }
        });
    }
}

/// O log não guarda o que foi dito, só uma marca para casar com o histórico.
fn hash(text: &str) -> String {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}
