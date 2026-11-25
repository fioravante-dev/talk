//! A fala: `talk-rs speak "texto"` → prosa → frases → Qwen3-TTS → `pw-play`.
//!
//! O mesmo desenho do ditado ao contrário. O serviço nunca toca na CUDA: quem
//! toca é um trabalhador em Python (o `faster-qwen3-tts` só existe lá), que sobe
//! na primeira fala e sai depois de ocioso (`speech_keep_alive`), devolvendo a
//! VRAM. As falas entram numa fila e saem uma depois da outra; `hush` (ou
//! começar uma ditada) corta a que está tocando e esvazia a fila.

use std::collections::HashMap;
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

/// O áudio pronto de uma fala: guardado para tocar no foco e para repetir.
#[derive(Clone)]
struct Clip {
    rate: u32,
    samples: Arc<Vec<f32>>,
}

/// Para onde vai uma fala: tocar enquanto gera, ou gerar inteira e guardar
/// até a conversa do T3 (pelo título) ganhar foco.
enum Target {
    Play,
    Hold(String),
}

/// Uma fala na fila, com a "geração" de quando entrou: um `hush` sobe a
/// geração, e tudo o que entrou antes dele é descartado.
struct Utterance {
    text: String,
    generation: u64,
    target: Target,
}

#[derive(Clone)]
pub struct Speech {
    queue: mpsc::Sender<Utterance>,
    generation: Arc<AtomicU64>,
    speaking: Arc<AtomicBool>,
    /// O `pw-play` tocando agora: o `stop` o mata e o som para na hora.
    player: Arc<Mutex<Option<Child>>>,
    /// Uma fala toca por vez: quem gera ao vivo e quem toca um áudio pronto
    /// (foco, repetir) passam por aqui.
    speaker: Arc<Mutex<()>>,
    worker: Arc<Mutex<Option<Worker>>>,
    /// O áudio pronto de cada conversa do T3 que espera a janela dela ganhar
    /// foco: só o último de cada uma, que o anterior já envelheceu.
    pending: Arc<Mutex<HashMap<String, (Clip, Instant)>>>,
    /// A última fala que tocou, para o "falar de novo" da bandeja.
    last: Arc<Mutex<Option<Clip>>>,
    /// O título da janela do T3 em foco, ou `None`.
    focused: Arc<Mutex<Option<String>>>,
    /// O vigia do foco subiu (é o COSMIC). Sem ele, `focus` vira `always`.
    watching: Arc<AtomicBool>,
}

/// Uma fala esperando o foco há mais que isto já não interessa.
const PENDING_TTL: Duration = Duration::from_secs(3600);

impl Speech {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<Utterance>();
        let me = Self {
            queue: tx,
            generation: Arc::new(AtomicU64::new(0)),
            speaking: Arc::new(AtomicBool::new(false)),
            player: Arc::new(Mutex::new(None)),
            speaker: Arc::new(Mutex::new(())),
            worker: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            last: Arc::new(Mutex::new(None)),
            focused: Arc::new(Mutex::new(None)),
            watching: Arc::new(AtomicBool::new(false)),
        };
        let generator = me.clone();
        thread::spawn(move || {
            for u in rx {
                if !generator.cancelled(u.generation)
                    && let Err(e) = generator.run(u)
                {
                    eprintln!("[speech] {e:#}");
                }
            }
        });
        me.reap();
        let watcher = me.clone();
        match crate::focus::watch(Box::new(move |w| watcher.on_focus(w))) {
            Ok(()) => me.watching.store(true, Ordering::SeqCst),
            Err(e) => eprintln!("[speech] sem vigia do foco ({e:#}): as falas tocam na hora"),
        }
        me
    }

    /// Põe na fila e volta logo: quem chama (o Claude) não espera a fala acabar.
    pub fn enqueue(&self, text: String) {
        self.push(text, Target::Play);
    }

    /// A fala de uma conversa do T3 (`thread` é o título dela). Com
    /// `speech_when = "focus"`, toca na hora só se a janela do T3 está em foco
    /// mostrando a conversa. Senão o áudio é gerado já (o modelo aquece) e
    /// guardado, e toca quando a conversa ganhar foco.
    pub fn enqueue_for_thread(&self, thread: String, text: String, when: &str) {
        if when != "focus" || !self.watching.load(Ordering::SeqCst) || self.shows(&thread) {
            self.enqueue(text);
        } else {
            self.push(text, Target::Hold(thread));
        }
    }

    fn push(&self, text: String, target: Target) {
        let generation = self.generation.load(Ordering::SeqCst);
        let _ = self.queue.send(Utterance { text, generation, target });
    }

    fn shows(&self, thread: &str) -> bool {
        lock(&self.focused).as_deref().is_some_and(|w| speech::window_shows_thread(w, thread))
    }

    /// O vigia avisou: o foco mudou. Se a janela agora mostra uma conversa
    /// com áudio esperando, ele toca.
    fn on_focus(&self, window: Option<&str>) {
        *lock(&self.focused) = window.map(str::to_owned);
        let Some(window) = window else { return };
        let ready: Vec<Clip> = {
            let mut pending = lock(&self.pending);
            let keys: Vec<String> =
                pending.keys().filter(|t| speech::window_shows_thread(window, t)).cloned().collect();
            keys.iter()
                .filter_map(|k| pending.remove(k))
                .filter(|(_, at)| at.elapsed() < PENDING_TTL)
                .map(|(clip, _)| clip)
                .collect()
        };
        for clip in ready {
            self.play_in_background(clip);
        }
    }

    /// "Falar de novo" (a bandeja, `talk-rs again`): a última fala que tocou.
    pub fn replay(&self) -> bool {
        let Some(clip) = lock(&self.last).clone() else { return false };
        self.play_in_background(clip);
        true
    }

    /// Quantas falas esperam o foco (para o `status`).
    pub fn waiting(&self) -> usize {
        lock(&self.pending).len()
    }

    /// `talk-rs hush`: cala e descarta também o que esperava o foco.
    pub fn hush(&self) {
        lock(&self.pending).clear();
        self.stop();
    }

    /// Corta o som agora e descarta a fila (começar uma ditada faz isto). O
    /// que espera o foco de uma conversa continua esperando.
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut p) = lock(&self.player).take() {
            let _ = p.kill();
            let _ = p.wait();
        }
    }

    pub fn speaking(&self) -> bool {
        self.speaking.load(Ordering::SeqCst)
    }

    /// Ocupado gerando conta como carregado: o `status` não espera a frase acabar.
    pub fn loaded(&self) -> bool {
        match self.worker.try_lock() {
            Ok(mut w) => w.as_mut().is_some_and(Worker::alive),
            Err(_) => true,
        }
    }

    fn cancelled(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) != generation
    }

    fn run(&self, u: Utterance) -> Result<()> {
        match u.target {
            Target::Play => {
                if let Some(clip) = self.synthesize(&u.text, u.generation, true)? {
                    *lock(&self.last) = Some(clip);
                }
            }
            Target::Hold(thread) => {
                let Some(clip) = self.synthesize(&u.text, u.generation, false)? else { return Ok(()) };
                if self.cancelled(u.generation) {
                    return Ok(());
                }
                // A conversa ganhou foco enquanto o áudio era gerado: toca já.
                if self.shows(&thread) {
                    self.play_clip(&clip, u.generation);
                } else {
                    eprintln!("[speech] pronto, esperando o foco: {}", hash(&thread));
                    let mut pending = lock(&self.pending);
                    pending.retain(|_, (_, at)| at.elapsed() < PENDING_TTL);
                    pending.insert(thread, (clip, Instant::now()));
                }
            }
        }
        Ok(())
    }

    /// Gera a fala inteira. Com `live`, toca enquanto gera (o 1º som sai em
    /// ~0,2 s); sem, só junta o áudio. `None` = cancelada ou vazia.
    fn synthesize(&self, text: &str, generation: u64, live: bool) -> Result<Option<Clip>> {
        let cfg = Config::load(&paths::config_file()).unwrap_or_default();
        let prose = speech::to_speech(text);
        if prose.is_empty() {
            return Ok(None);
        }
        let language = speech::detect_language(&prose);
        let speaker = cfg.speech_voice(language).to_owned();
        let started = Instant::now();
        let mut first_sound: Option<Duration> = None;
        let mut samples: Vec<f32> = vec![];
        let mut rate = 0;
        let mut out: Option<ChildStdin> = None;
        // Ao vivo, segura o alto-falante a fala toda: um áudio pronto que
        // chegar no meio espera a vez.
        let _turn = live.then(|| lock(&self.speaker));
        if live {
            self.speaking.store(true, Ordering::SeqCst);
        }

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
                rate = chunk.rate;
                samples.extend_from_slice(&audio);
                if live {
                    if out.is_none() && first_sound.is_none() {
                        out = Some(self.open_player(rate)?);
                        first_sound = Some(started.elapsed());
                    }
                    if out.as_mut().is_some_and(|o| o.write_all(&f32_bytes(&audio)).is_err()) {
                        out = None; // o pw-play morreu (stop): o resto é descartado
                    }
                }
            }
            w.last_used = Instant::now();
        }
        if live {
            drop(out);
            self.wait_player();
            self.speaking.store(false, Ordering::SeqCst);
        }
        eprintln!(
            "[speech] {} ({language}, {speaker}): {}, {:.1}s de áudio em {:.1}s",
            hash(&prose),
            first_sound.map_or("guardado".to_owned(), |t| format!("1º som {:.2}s", t.as_secs_f64())),
            samples.len() as f64 / f64::from(rate.max(1)),
            started.elapsed().as_secs_f64()
        );
        if self.cancelled(generation) || samples.is_empty() {
            return Ok(None);
        }
        Ok(Some(Clip { rate, samples: Arc::new(samples) }))
    }

    /// Toca um áudio pronto numa thread: quem avisa (o vigia, a bandeja) não espera.
    fn play_in_background(&self, clip: Clip) {
        let me = self.clone();
        let generation = self.generation.load(Ordering::SeqCst);
        thread::spawn(move || me.play_clip(&clip, generation));
    }

    /// Toca um áudio pronto, na vez dele; vira a "última fala".
    fn play_clip(&self, clip: &Clip, generation: u64) {
        let _turn = lock(&self.speaker);
        if self.cancelled(generation) {
            return;
        }
        self.speaking.store(true, Ordering::SeqCst);
        match self.open_player(clip.rate) {
            Ok(mut out) => {
                let _ = out.write_all(&f32_bytes(&clip.samples));
                drop(out);
                self.wait_player();
                *lock(&self.last) = Some(clip.clone());
            }
            Err(e) => eprintln!("[speech] {e:#}"),
        }
        self.speaking.store(false, Ordering::SeqCst);
    }

    /// Espera o `pw-play` tocar o que falta, sem segurar o lock: um `stop` no
    /// meio ainda o mata.
    fn wait_player(&self) {
        loop {
            let mut p = lock(&self.player);
            match p.as_mut().map(Child::try_wait) {
                Some(Ok(None)) => {}
                _ => {
                    *p = None;
                    return;
                }
            }
            drop(p);
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Um `pw-play` cru, mono, f32, na taxa do modelo. Volta a entrada dele.
    fn open_player(&self, rate: u32) -> Result<ChildStdin> {
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

fn f32_bytes(samples: &[f32]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

/// O log não guarda o que foi dito, só uma marca para casar com o histórico.
fn hash(text: &str) -> String {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}
