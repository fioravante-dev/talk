//! O serviço: atalho → grava → trabalhador → cola, e a bandeja dizendo onde está.
//!
//! Ele nunca toca na CUDA; quem toca é o trabalhador, um processo filho que
//! começa a carregar o modelo no instante em que a gravação começa e termina
//! depois de ocioso (`whisper_keep_alive`), devolvendo a VRAM inteira.
//!
//! Config e vocabulário são relidos a cada ditada: editar o `vocab.txt` vale na
//! próxima frase, sem reiniciar nada.

use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use talk_core::config::Config;
use talk_core::vocab::Vocab;
use talk_core::{history, paths, wire::Job};
#[cfg(windows)]
use uds_windows::{UnixListener, UnixStream};

use crate::audio::{Capture, RATE};
use crate::output::{self, Keyboard};
use crate::tray::{Action, Handle, Look, Tray, TrayMethods};
use crate::worker::Worker;

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub cmd: String,
    /// Só no `toggle`: "pt", "en", "auto"; ausente = o do config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_loaded: Option<bool>,
    /// Só no Windows: a RAM do próprio serviço, que lá a janela não tem como
    /// ler barato (no Linux ela lê o `/proc`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rss_mb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn event_phase(phase: &str, seconds: u64) -> String {
    serde_json::json!({"event": "phase", "phase": phase, "seconds": seconds}).to_string()
}

enum Phase {
    Idle,
    Recording { capture: Capture, language: Option<String> },
    Transcribing,
}

impl Phase {
    fn name(&self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Recording { .. } => "recording",
            Phase::Transcribing => "transcribing",
        }
    }
}

pub struct Service {
    phase: Mutex<Phase>,
    worker: Mutex<Option<Worker>>,
    keyboard: Mutex<Keyboard>,
    last: Mutex<String>,
    tray: Mutex<Option<Handle<Tray>>>,
    /// As janelas abertas, ouvindo os eventos. Quem fechou sai na próxima escrita.
    subscribers: Mutex<Vec<UnixStream>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(unix)]
fn notify(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "talk", "-t", "2500", title, body])
        .spawn();
}

/// No Windows, a notificação nativa (toast). Mostrar leva alguns ms de COM:
/// numa thread, para não atrasar a colagem.
#[cfg(windows)]
fn notify(title: &str, body: &str) {
    eprintln!("[talk] {title} {body}");
    let (title, body) = (title.to_owned(), body.to_owned());
    thread::spawn(move || {
        let _ = notify_rust::Notification::new().summary(&title).body(&body).show();
    });
}

fn log_file() -> PathBuf {
    paths::cache_dir().join("talk-rs.log")
}

fn model_path(cfg: &Config) -> PathBuf {
    let p = PathBuf::from(&cfg.ggml_model);
    if p.is_absolute() { p } else { paths::models_dir().join(p) }
}

/// "pt", "en" → fixa; "auto"/vazio → detecta; ausente → o do config.
pub fn resolve_language(asked: Option<&str>, cfg: &Config) -> String {
    match asked.map(str::trim) {
        Some("auto") | Some("") => String::new(),
        Some(l) => l.to_owned(),
        None => cfg.language.clone(),
    }
}

impl Service {
    fn broadcast(&self, line: &str) {
        lock(&self.subscribers).retain(|s| {
            let mut out = s;
            writeln!(out, "{line}").is_ok()
        });
    }

    fn set_look(&self, look: Look, tooltip: impl Into<String>) {
        let phase = match look {
            Look::Idle => "idle",
            Look::Recording { .. } => "recording",
            Look::Transcribing => "transcribing",
        };
        if !matches!(look, Look::Recording { bright: false }) {
            self.broadcast(&event_phase(phase, 0));
        }
        let tooltip = tooltip.into();
        if let Some(h) = lock(&self.tray).as_ref() {
            h.update(|t| {
                t.look = look;
                t.tooltip = tooltip;
            });
        }
    }

    /// Sobe o trabalhador se não houver um vivo com o modelo certo. Volta logo:
    /// a carga acontece no processo filho, em paralelo à fala.
    fn ensure_worker(&self, cfg: &Config) -> Result<()> {
        let model = model_path(cfg);
        if !model.exists() {
            bail!("modelo não encontrado: {}", model.display());
        }
        let mut w = lock(&self.worker);
        let ok = w.as_mut().is_some_and(|w| w.alive() && w.model == model);
        if !ok {
            *w = None; // o antigo sai antes, para os dois não disputarem a VRAM
            *w = Some(Worker::spawn(&model, &log_file())?);
        }
        Ok(())
    }

    pub fn toggle(self: &Arc<Self>, language: Option<String>) -> Result<&'static str> {
        let mut phase = lock(&self.phase);
        match std::mem::replace(&mut *phase, Phase::Transcribing) {
            Phase::Idle => {
                let cfg = Config::load(&paths::config_file()).unwrap_or_default();
                let target = (!cfg.input_device.is_empty()).then_some(cfg.input_device.as_str());
                let capture = match Capture::start(target) {
                    Ok(c) => c,
                    Err(e) => {
                        *phase = Phase::Idle;
                        return Err(e);
                    }
                };
                *phase = Phase::Recording { capture, language };
                drop(phase);
                // A pílula ouve os eventos sozinha e sai quando a fase volta a
                // pronto; sem layer-shell ou sem o binário, segue sem ela.
                if let Ok(exe) = std::env::current_exe()
                    && let Ok(mut pill) = std::process::Command::new(sibling(&exe, "talk-pill"))
                        .stderr(std::process::Stdio::null())
                        .spawn()
                {
                    // Recolhe a pílula quando ela sai; sem o `wait`, cada
                    // ditada deixava um processo zumbi.
                    thread::spawn(move || pill.wait());
                }
                self.set_look(Look::Recording { bright: true }, "gravando");
                let me = self.clone();
                thread::spawn(move || {
                    let cfg = Config::load(&paths::config_file()).unwrap_or_default();
                    if let Err(e) = me.ensure_worker(&cfg) {
                        notify("⚠ talk", &format!("{e:#}"));
                    }
                });
                self.pulse();
                Ok("recording")
            }
            Phase::Recording { capture, language } => {
                drop(phase);
                self.set_look(Look::Transcribing, "transcrevendo");
                let me = self.clone();
                thread::spawn(move || {
                    if let Err(e) = me.finish(capture, language.as_deref()) {
                        notify("⚠ talk", &format!("{e:#}"));
                    }
                    *lock(&me.phase) = Phase::Idle;
                    me.set_look(Look::Idle, "pronto");
                });
                Ok("transcribing")
            }
            Phase::Transcribing => Ok("transcribing"),
        }
    }

    /// Enquanto grava: o ponto vermelho pulsa a cada 500 ms, com o cronômetro
    /// na dica, e as janelas recebem o nível do microfone a cada 100 ms.
    fn pulse(self: &Arc<Self>) {
        let me = self.clone();
        thread::spawn(move || {
            let mut bright = true;
            for tick in 1u64.. {
                thread::sleep(Duration::from_millis(100));
                let (secs, level) = match &*lock(&me.phase) {
                    Phase::Recording { capture, .. } => (capture.started.elapsed().as_secs(), capture.level()),
                    _ => return,
                };
                me.broadcast(
                    &serde_json::json!({"event": "level", "level": level, "seconds": secs}).to_string(),
                );
                if tick % 5 == 0 {
                    bright = !bright;
                    me.set_look(Look::Recording { bright }, format!("gravando {}:{:02}", secs / 60, secs % 60));
                }
            }
        });
    }

    fn finish(&self, capture: Capture, language: Option<&str>) -> Result<()> {
        let released = Instant::now();
        let audio = capture.stop()?;
        let cfg = Config::load(&paths::config_file())?;
        let vocab_text = std::fs::read_to_string(paths::vocab_file()).unwrap_or_default();
        let vocab = Vocab::new(&vocab_text, &cfg.vocab);
        let job = Job {
            language: resolve_language(language, &cfg),
            prompt: vocab.prompt(),
            beam_size: cfg.beam_size,
            samples: audio.len(),
        };

        self.ensure_worker(&cfg)?;
        let done = {
            let mut w = lock(&self.worker);
            let worker = w.as_mut().ok_or_else(|| anyhow!("sem trabalhador"))?;
            match worker.transcribe(&job, &audio) {
                Ok(d) => d,
                Err(e) => {
                    *w = None; // morreu no meio: o próximo atalho sobe outro
                    return Err(e).context("o trabalhador caiu (veja ~/.cache/talk/talk-rs.log)");
                }
            }
        };
        if let Some(err) = done.error {
            bail!("{err}");
        }
        let text = vocab.apply(&done.text);
        if text.is_empty() {
            notify("⃠ Nenhuma fala reconhecida", "");
            return Ok(());
        }

        let delivered: &[&str] = match cfg.to.as_str() {
            "stdout" => {
                eprintln!("[talk] texto: {text}");
                &["stdout"]
            }
            "clipboard" => {
                output::copy(&text)?;
                &["clipboard"]
            }
            _ => {
                output::copy(&text)?;
                let combo = output::parse_combo(&cfg.paste_key)?;
                lock(&self.keyboard).press(&combo)?;
                &["paste"]
            }
        };
        let delivered_s = released.elapsed().as_secs_f64();
        *lock(&self.last) = text.clone();

        history::append(
            &paths::history_file(),
            &history::Line {
                at: history::now(),
                text: &text,
                language: &done.language,
                motor: "whisper.cpp",
                delivered,
                audio_seconds: audio.len() as f64 / RATE as f64,
                stt_seconds: done.stt_seconds,
            },
        )?;
        eprintln!(
            "[talk] {:.1}s de áudio, STT {:.2}s, soltar→colado {:.2}s",
            audio.len() as f64 / RATE as f64,
            done.stt_seconds,
            delivered_s
        );
        self.broadcast(r#"{"event":"dictation"}"#);
        if cfg.keep_alive()? == Some(Duration::ZERO) {
            *lock(&self.worker) = None;
        }
        Ok(())
    }

    /// Manda o trabalhador embora depois de ocioso: é aqui que a VRAM volta a 0.
    fn reap(self: &Arc<Self>) {
        let me = self.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(5));
                if !matches!(&*lock(&me.phase), Phase::Idle) {
                    continue;
                }
                let Ok(Some(ttl)) = Config::load(&paths::config_file()).and_then(|c| c.keep_alive())
                else {
                    continue;
                };
                let mut w = lock(&me.worker);
                if w.as_ref().is_some_and(|w| w.idle_for() >= ttl) {
                    *w = None;
                }
            }
        });
    }

    fn handle(self: &Arc<Self>, stream: UnixStream) -> Result<()> {
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        if line.trim().is_empty() {
            return Ok(()); // só uma sonda de "tem alguém aí?", como a do serve
        }
        let req: Request = serde_json::from_str(line.trim()).context("pedido inválido")?;
        let reply = match req.cmd.as_str() {
            "toggle" => match self.toggle(req.language) {
                Ok(phase) => Reply { ok: true, phase: phase.into(), ..Reply::default() },
                Err(e) => {
                    notify("⚠ talk", &format!("{e:#}"));
                    Reply {
                        ok: false,
                        phase: lock(&self.phase).name().into(),
                        error: Some(format!("{e:#}")),
                        ..Reply::default()
                    }
                }
            },
            "status" => Reply {
                ok: true,
                phase: lock(&self.phase).name().into(),
                pid: Some(std::process::id()),
                model_loaded: Some(lock(&self.worker).as_mut().is_some_and(Worker::alive)),
                #[cfg(windows)]
                rss_mb: memory_stats::memory_stats().map(|m| m.physical_mem as f64 / 1_048_576.0),
                ..Reply::default()
            },
            "subscribe" => {
                // A conexão fica aberta: daqui em diante ela recebe os eventos.
                let mut out = &stream;
                writeln!(out, "{}", event_phase(lock(&self.phase).name(), 0))?;
                lock(&self.subscribers).push(stream);
                return Ok(());
            }
            other => Reply { ok: false, error: Some(format!("comando desconhecido: {other}")), ..Reply::default() },
        };
        let mut out = &stream;
        writeln!(out, "{}", serde_json::to_string(&reply)?)?;
        Ok(())
    }
}

/// Um binário do talk ao lado deste (`talk-ui`, `talk-pill`), com o `.exe` no Windows.
fn sibling(exe: &std::path::Path, name: &str) -> PathBuf {
    exe.with_file_name(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// A janela é outro binário (`talk-ui`), ao lado deste: o serviço nunca carrega
/// o motor web. No Linux ela só existe enquanto está aberta; no Windows ela
/// fica escondida em segundo plano, e isto só a traz para a frente.
pub fn open_window() -> Result<()> {
    let exe = sibling(&std::env::current_exe()?, "talk-ui");
    std::process::Command::new(&exe)
        .spawn()
        .with_context(|| format!("não deu para abrir {}", exe.display()))?;
    Ok(())
}

/// No Windows um programa de console sempre ganha uma janela; o serviço sobe
/// sem nenhuma (`CREATE_NO_WINDOW`), solto de quem o chamou.
#[cfg(windows)]
pub fn start_detached() -> Result<()> {
    use std::os::windows::process::CommandExt;
    if UnixStream::connect(paths::socket()).is_ok() {
        bail!("já existe um talk-rs rodando");
    }
    std::fs::create_dir_all(paths::cache_dir())?;
    let log = paths::cache_dir().join("serve.log");
    let err = std::fs::OpenOptions::new().create(true).append(true).open(&log)?;
    let child = std::process::Command::new(std::env::current_exe()?)
        .arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(err)
        .creation_flags(crate::worker::NO_WINDOW | crate::worker::DETACHED)
        .spawn()
        .context("não deu para subir o serviço")?;
    println!("talk-rs rodando (pid {}), log em {}", child.id(), log.display());
    Ok(())
}

/// Só no Windows: liga ou desliga o início no login (`HKCU\...\Run`),
/// apontando para o `talk-start.exe` ao lado deste, que sobe sem janela.
#[cfg(windows)]
pub fn autostart(on: bool) -> Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let (run, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .context("não deu para abrir o registro")?;
    if on {
        let start = std::env::current_exe()?.with_file_name("talk-start.exe");
        if !start.exists() {
            bail!("{} não existe", start.display());
        }
        run.set_value("talk", &format!("\"{}\"", start.display()))?;
        println!("o talk sobe no login: {}", start.display());
    } else {
        match run.delete_value("talk") {
            Ok(()) => println!("o talk não sobe mais no login"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => println!("já não subia no login"),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub fn serve() -> Result<()> {
    let sock = paths::socket();
    if UnixStream::connect(&sock).is_ok() {
        bail!("já existe um talk-rs rodando ({})", sock.display());
    }
    let _ = std::fs::remove_file(&sock);
    std::fs::create_dir_all(paths::cache_dir())?;
    let listener = UnixListener::bind(&sock).with_context(|| sock.display().to_string())?;

    let service = Arc::new(Service {
        phase: Mutex::new(Phase::Idle),
        worker: Mutex::new(None),
        keyboard: Mutex::new(Keyboard::new()?),
        last: Mutex::new(String::new()),
        tray: Mutex::new(None),
        subscribers: Mutex::new(vec![]),
    });

    let (tx, rx) = mpsc::channel();
    // No login o serviço pode subir antes do painel: sem quem receba o ícone,
    // o registro falha. Tenta de novo a cada 2 s até o painel aparecer.
    let me = service.clone();
    thread::spawn(move || {
        for attempt in 0u32.. {
            match (Tray { look: Look::Idle, tooltip: "pronto".into(), actions: tx.clone() }).spawn() {
                Ok(h) => {
                    if attempt > 0 {
                        eprintln!("[talk] bandeja de pé (tentativa {})", attempt + 1);
                    }
                    *lock(&me.tray) = Some(h);
                    return;
                }
                Err(e) => {
                    if attempt == 0 {
                        eprintln!("[talk] sem bandeja ainda, tentando de novo: {e}");
                    }
                    thread::sleep(Duration::from_secs(2));
                }
            }
        }
    });
    let me = service.clone();
    thread::spawn(move || {
        for action in rx {
            match action {
                Action::Toggle => {
                    if let Err(e) = me.toggle(None) {
                        notify("⚠ talk", &format!("{e:#}"));
                    }
                }
                Action::Open => {
                    if let Err(e) = open_window() {
                        notify("⚠ talk", &format!("{e:#}"));
                    }
                }
                Action::CopyLast => {
                    let last = lock(&me.last).clone();
                    if !last.is_empty() {
                        let _ = output::copy(&last);
                    }
                }
                Action::Quit => {
                    eprintln!("[talk] saindo (pela bandeja)");
                    let _ = std::fs::remove_file(paths::socket());
                    std::process::exit(0);
                }
            }
        }
    });
    service.reap();
    #[cfg(windows)]
    crate::hotkeys::spawn(service.clone(), &Config::load(&paths::config_file()).unwrap_or_default());

    // A janela sobe já, escondida: o WebView2 leva ~1 s para acordar a frio,
    // e assim abrir pela bandeja é só mostrar.
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(sibling(&exe, "talk-ui")).arg("--hidden").spawn();
    }

    eprintln!("[talk] ouvindo em {}", sock.display());
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let me = service.clone();
        thread::spawn(move || {
            if let Err(e) = me.handle(stream) {
                eprintln!("[talk] {e:#}");
            }
        });
    }
    Ok(())
}

pub fn request(req: &Request) -> Result<Reply> {
    let sock = paths::socket();
    let stream = UnixStream::connect(&sock)
        .with_context(|| format!("o serviço não está rodando ({}): talk-rs serve", sock.display()))?;
    let mut out = &stream;
    writeln!(out, "{}", serde_json::to_string(req)?)?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    serde_json::from_str(line.trim()).context("resposta inválida do serviço")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shortcut_language_wins_over_the_config() {
        let cfg = Config { language: "pt".into(), ..Config::default() };
        assert_eq!(resolve_language(Some("en"), &cfg), "en");
        assert_eq!(resolve_language(Some("auto"), &cfg), "");
        assert_eq!(resolve_language(None, &cfg), "pt");
    }
}
