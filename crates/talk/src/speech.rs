//! A fala: `talk-rs speak "texto"` → prosa → frases → Qwen3-TTS → alto-falante
//! (`pw-play` no Linux, WASAPI no Windows: `player.rs`/`player_win.rs`).
//!
//! O mesmo desenho do ditado ao contrário. O serviço nunca toca na CUDA: quem
//! toca é um trabalhador em Python (o `faster-qwen3-tts` só existe lá), que sobe
//! na primeira fala e sai depois de ocioso (`speech_keep_alive`), devolvendo a
//! VRAM. As falas entram numa fila e saem uma depois da outra; `hush` (ou
//! começar uma ditada) corta a que está tocando e esvazia a fila.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::player::{self, Player};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use talk_core::config::Config;
use talk_core::paths;
use talk_core::speech::{self, Say};
use talk_core::spoken::{self, Spoken};

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
        let venv = speech_dir().join("venv");
        if cfg!(windows) { venv.join(r"Scripts\python.exe") } else { venv.join("bin/python") }
    } else {
        PathBuf::from(&cfg.speech_python)
    }
}

/// O processo Python, visto daqui.
/// O ambiente da fala está instalado (o Python do `install`).
pub fn installed(cfg: &Config) -> bool {
    python(cfg).exists()
}

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
            anyhow::bail!("sem o ambiente da fala ({}): instale com TALK_SPEECH=1", py.display());
        }
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("speech.log"))?;
        let mut cmd = Command::new(&py);
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, crate::worker::NO_WINDOW);
        let mut child = cmd
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

/// O áudio pronto de uma fala, já no histórico (`id`).
#[derive(Clone)]
struct Clip {
    id: String,
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
    /// O título da conversa do T3 de onde veio, para o histórico.
    thread: Option<String>,
    /// Gerada de novo para uma fala do histórico cujo áudio já saiu: grava
    /// o áudio com o id dela, sem linha nova.
    redo: Option<String>,
}

#[derive(Clone)]
pub struct Speech {
    queue: mpsc::Sender<Utterance>,
    generation: Arc<AtomicU64>,
    speaking: Arc<AtomicBool>,
    /// O som tocando agora: o `stop` o corta na hora.
    player: Arc<Mutex<Option<Player>>>,
    /// Uma fala toca por vez: quem gera ao vivo e quem toca um áudio pronto
    /// (foco, repetir) passam por aqui.
    speaker: Arc<Mutex<()>>,
    worker: Arc<Mutex<Option<Worker>>>,
    /// O áudio pronto de cada conversa do T3 que espera a janela dela ganhar
    /// foco: só o último de cada uma, que o anterior já envelheceu.
    pending: Arc<Mutex<HashMap<String, (Clip, Instant)>>>,
    /// O id da última fala que tocou, para o "falar de novo" da bandeja.
    last: Arc<Mutex<Option<String>>>,
    /// Quem quer saber de cada fala nova no histórico (a bandeja).
    on_spoken: Arc<Mutex<Option<OnSpoken>>>,
    /// O título da janela do T3 em foco, ou `None`.
    focused: Arc<Mutex<Option<String>>>,
    /// O vigia do foco subiu (é o COSMIC). Sem ele, `focus` vira `always`.
    watching: Arc<AtomicBool>,
    /// Gravando uma ditada: nada toca (o microfone pegaria a fala), e o que
    /// ficar pronto espera, guardado.
    dictating: Arc<AtomicBool>,
    /// Ninguém mexe no PC há uns 30 s (`focus::IDLE_MS`).
    idle: Arc<AtomicBool>,
    /// A última vez que a pessoa estava aqui, com o T3 na frente: com dois
    /// PCs, a fala vai para o que ela usou por último.
    present_at: Arc<Mutex<Option<Instant>>>,
}

type OnSpoken = Box<dyn Fn() + Send>;

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
            on_spoken: Arc::new(Mutex::new(None)),
            focused: Arc::new(Mutex::new(None)),
            watching: Arc::new(AtomicBool::new(false)),
            dictating: Arc::new(AtomicBool::new(false)),
            idle: Arc::new(AtomicBool::new(false)),
            present_at: Arc::new(Mutex::new(None)),
        };
        let generator = me.clone();
        thread::spawn(move || {
            for u in rx {
                // Calada antes de começar, a fala ainda é gerada e guardada (em
                // silêncio): fica no histórico para tocar depois.
                if let Err(e) = generator.run(u) {
                    eprintln!("[speech] {e:#}");
                }
            }
        });
        me.reap();
        let watcher = me.clone();
        match crate::focus::watch(Box::new(move |seen| match seen {
            crate::focus::Seen::Focus(w) => watcher.on_focus(w),
            crate::focus::Seen::Idle(away) => watcher.on_idle(away),
        })) {
            Ok(()) => me.watching.store(true, Ordering::SeqCst),
            Err(e) => eprintln!("[speech] sem vigia do foco ({e:#}): as falas tocam na hora"),
        }
        me
    }

    /// Põe na fila e volta logo: quem chama (o Claude) não espera a fala acabar.
    pub fn enqueue(&self, text: String) {
        self.push(text, Target::Play, None);
    }

    /// Chamado a cada fala nova no histórico.
    pub fn on_spoken(&self, f: impl Fn() + Send + 'static) {
        *lock(&self.on_spoken) = Some(Box::new(f));
    }

    /// A fala de uma conversa do T3 (`thread` é o título dela). Com
    /// `speech_when = "focus"`, toca na hora só se a janela do T3 está em foco
    /// mostrando a conversa. Senão o áudio é gerado já (o modelo aquece) e
    /// guardado, e toca quando a conversa ganhar foco.
    pub fn enqueue_for_thread(&self, thread: String, text: String, when: &str) {
        if when != "focus" || !self.watching.load(Ordering::SeqCst) || self.shows(&thread) {
            self.push(text, Target::Play, Some(thread));
        } else {
            self.push(text, Target::Hold(thread.clone()), Some(thread));
        }
    }

    fn push(&self, text: String, target: Target, thread: Option<String>) {
        let generation = self.generation.load(Ordering::SeqCst);
        let _ = self.queue.send(Utterance { text, generation, target, thread, redo: None });
    }

    fn shows(&self, thread: &str) -> bool {
        lock(&self.focused).as_deref().is_some_and(|w| speech::window_shows_thread(w, thread))
    }

    /// O vigia avisou: o foco mudou. Se a janela agora mostra uma conversa
    /// com áudio esperando, ele toca.
    fn on_focus(&self, window: Option<&str>) {
        self.mark_presence();
        *lock(&self.focused) = window.map(str::to_owned);
        let Some(window) = window else { return };
        if self.dictating() {
            return; // espera a ditada; `set_dictating(false)` olha de novo
        }
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

    /// "Falar de novo" (o ícone da bandeja, `talk-rs again`): a última fala
    /// que tocou; depois de reiniciar, a mais nova do histórico.
    pub fn replay(&self) -> bool {
        let id = lock(&self.last).clone().or_else(|| spoken::read(&spoken::file(), 1).pop().map(|s| s.id));
        id.is_some_and(|id| self.play_entry(&id))
    }

    /// Toca uma fala do histórico: o áudio guardado, ou, se ele já saiu, a
    /// fala gerada de novo a partir do texto. `false` = id desconhecido.
    pub fn play_entry(&self, id: &str) -> bool {
        let Some(entry) = spoken::find(&spoken::file(), id) else { return false };
        let path = spoken::clip_path(id);
        if path.exists() {
            let me = self.clone();
            let generation = self.generation.load(Ordering::SeqCst);
            let id = id.to_owned();
            thread::spawn(move || me.play_file(&path, &id, generation));
        } else {
            let generation = self.generation.load(Ordering::SeqCst);
            let thread = (!entry.thread.is_empty()).then_some(entry.thread);
            let _ = self.queue.send(Utterance {
                text: entry.text,
                generation,
                target: Target::Play,
                thread,
                redo: Some(entry.id),
            });
        }
        true
    }

    /// Quantas falas esperam o foco (para o `status`).
    fn on_idle(&self, away: bool) {
        self.mark_presence();
        self.idle.store(away, Ordering::SeqCst);
    }

    fn present_now(&self) -> bool {
        lock(&self.focused).is_some() && !self.idle.load(Ordering::SeqCst)
    }

    /// Antes de cada mudança de foco ou de largado: se a pessoa estava aqui
    /// até agora, anota.
    fn mark_presence(&self) {
        if self.present_now() {
            *lock(&self.present_at) = Some(Instant::now());
        }
    }

    /// Há quanto tempo a pessoa usou este PC com o T3 na frente: 0 se agora,
    /// `None` se nunca (desde que o serviço subiu) ou sem vigia do foco.
    pub fn presence(&self) -> Option<Duration> {
        if self.present_now() {
            return Some(Duration::ZERO);
        }
        lock(&self.present_at).map(|t| t.elapsed())
    }

    /// A ditada começou ou acabou. No começo cala o que estiver tocando; no
    /// fim, toca o que ficou esperando pela conversa que está na tela.
    pub fn set_dictating(&self, on: bool) {
        self.dictating.store(on, Ordering::SeqCst);
        if on {
            self.stop();
        } else {
            let focused = lock(&self.focused).clone();
            self.on_focus(focused.as_deref());
        }
    }

    fn dictating(&self) -> bool {
        self.dictating.load(Ordering::SeqCst)
    }

    pub fn waiting(&self) -> usize {
        lock(&self.pending).len()
    }

    /// `talk-rs hush`: cala e descarta também o que esperava o foco.
    pub fn hush(&self) {
        lock(&self.pending).clear();
        self.stop();
    }

    /// Corta o som agora (começar uma ditada faz isto). A fila não toca, mas
    /// é gerada em silêncio e vai para o histórico; o que espera o foco de
    /// uma conversa continua esperando.
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.speaking.store(false, Ordering::SeqCst);
        if let Some(mut p) = lock(&self.player).take() {
            p.kill();
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
        let live = matches!(u.target, Target::Play) && !self.cancelled(u.generation);
        let Some((samples, rate, made)) = self.synthesize(&u.text, u.generation, live)? else { return Ok(()) };
        let clip = self.record(samples, rate, made, u.thread.as_deref(), u.redo)?;
        match u.target {
            Target::Play => *lock(&self.last) = Some(clip.id),
            Target::Hold(thread) => {
                // A conversa ganhou foco enquanto o áudio era gerado: toca já.
                // Um `stop` (a ditada) não joga fora a fala que espera o foco;
                // o `hush` esvazia as que esperam.
                if self.shows(&thread) && !self.dictating() {
                    self.play_clip(&clip, self.generation.load(Ordering::SeqCst));
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

    /// Guarda a fala no histórico: o texto numa linha, o áudio num WAV (e
    /// limpa os áudios velhos). `redo` regrava o áudio de uma fala antiga.
    fn record(&self, samples: Vec<f32>, rate: u32, made: Made, thread: Option<&str>, redo: Option<String>) -> Result<Clip> {
        let cfg = Config::load(&paths::config_file()).unwrap_or_default();
        let fresh = redo.is_none();
        let id = redo.unwrap_or_else(spoken::new_id);
        spoken::write_wav(&spoken::clip_path(&id), rate, &samples)?;
        if fresh {
            spoken::append(
                &spoken::file(),
                &Spoken {
                    id: id.clone(),
                    at: talk_core::history::now(),
                    thread: thread.unwrap_or_default().to_owned(),
                    language: made.language.into(),
                    voice: made.voice,
                    seconds: samples.len() as f64 / f64::from(rate.max(1)),
                    text: made.prose,
                },
            )?;
        }
        let keep = cfg.speech_audio_keep().unwrap_or(Some(Duration::from_secs(7 * 86_400)));
        if let Err(e) = spoken::prune_clips(&spoken::clips_dir(), keep, cfg.speech_audio_max_mb * 1_048_576) {
            eprintln!("[speech] limpando os áudios: {e:#}");
        }
        if fresh && let Some(f) = lock(&self.on_spoken).as_ref() {
            f();
        }
        Ok(Clip { id, rate, samples: Arc::new(samples) })
    }

    /// Gera a fala inteira. Com `live`, toca enquanto gera (o 1º som sai em
    /// ~0,2 s); sem, só junta o áudio. `None` = cancelada ou vazia.
    fn synthesize(&self, text: &str, generation: u64, live: bool) -> Result<Option<(Vec<f32>, u32, Made)>> {
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
        let mut out: Option<player::Sink> = None;
        // Ao vivo, segura o alto-falante a fala toda: um áudio pronto que
        // chegar no meio espera a vez.
        // Calada no meio (`stop`, a ditada), para de tocar mas termina de
        // gerar: a fala inteira vai para o histórico.
        let mut audible = live && !self.dictating();
        let mut turn = audible.then(|| lock(&self.speaker));
        if audible {
            self.speaking.store(true, Ordering::SeqCst);
        }

        for sentence in speech::sentences(&prose, MAX_SENTENCE) {
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
                if audio.is_empty() {
                    continue;
                }
                rate = chunk.rate;
                samples.extend_from_slice(&audio);
                if audible && (self.cancelled(generation) || self.dictating()) {
                    audible = false;
                    out = None;
                    turn = None; // solta o alto-falante para quem vier
                    self.speaking.store(false, Ordering::SeqCst);
                }
                if audible {
                    if out.is_none() && first_sound.is_none() {
                        out = Some(self.open_player(rate)?);
                        first_sound = Some(started.elapsed());
                    }
                    if out.as_mut().is_some_and(|o| o.write(&audio).is_err()) {
                        out = None; // cortado (stop)
                    }
                }
            }
            w.last_used = Instant::now();
        }
        if audible {
            drop(out);
            self.wait_player();
            self.speaking.store(false, Ordering::SeqCst);
        }
        drop(turn);
        eprintln!(
            "[speech] {} ({language}, {speaker}): {}, {:.1}s de áudio em {:.1}s",
            hash(&prose),
            first_sound.map_or("guardado".to_owned(), |t| format!("1º som {:.2}s", t.as_secs_f64())),
            samples.len() as f64 / f64::from(rate.max(1)),
            started.elapsed().as_secs_f64()
        );
        if samples.is_empty() {
            return Ok(None);
        }
        Ok(Some((samples, rate, Made { prose, language, voice: speaker })))
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
        self.play_samples(clip.rate, &clip.samples, &clip.id);
        self.speaking.store(false, Ordering::SeqCst);
    }

    fn play_samples(&self, rate: u32, samples: &[f32], id: &str) {
        match self.open_player(rate) {
            Ok(mut out) => {
                let _ = out.write(samples);
                drop(out);
                self.wait_player();
                *lock(&self.last) = Some(id.to_owned());
            }
            Err(e) => eprintln!("[speech] {e:#}"),
        }
    }

    /// Toca o WAV de uma fala do histórico, na vez dele.
    fn play_file(&self, path: &std::path::Path, id: &str, generation: u64) {
        let _turn = lock(&self.speaker);
        if self.cancelled(generation) {
            return;
        }
        self.speaking.store(true, Ordering::SeqCst);
        match spoken::read_wav(path) {
            Ok((rate, samples)) => self.play_samples(rate, &samples, id),
            Err(e) => eprintln!("[speech] {}: {e:#}", path.display()),
        }
        self.speaking.store(false, Ordering::SeqCst);
    }

    /// Espera tocar o que falta, sem segurar o lock: um `stop` no meio ainda
    /// corta.
    fn wait_player(&self) {
        loop {
            let mut p = lock(&self.player);
            match p.as_mut().map(Player::finished) {
                Some(false) => {}
                _ => {
                    *p = None;
                    return;
                }
            }
            drop(p);
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// O alto-falante, mono, na taxa do modelo. Volta a entrada dele.
    fn open_player(&self, rate: u32) -> Result<player::Sink> {
        let (p, sink) = player::open(rate)?;
        *lock(&self.player) = Some(p);
        Ok(sink)
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

/// O que a síntese decidiu, para o histórico.
struct Made {
    prose: String,
    language: &'static str,
    voice: String,
}

/// O log não guarda o que foi dito, só uma marca para casar com o histórico.
fn hash(text: &str) -> String {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}
