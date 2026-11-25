//! `talk-rs`: o ditado do talk, em Rust.
//!
//!     talk-rs serve            o serviço (bandeja, atalhos, colagem)
//!     talk-rs toggle [-l pt]   o que o atalho chama: começa ou termina a ditada
//!     talk-rs speak "texto"   fala (o Claude chama no fim de cada resposta)
//!     talk-rs hush             cala
//!     talk-rs again            fala de novo a última fala
//!     talk-rs status
//!     talk-rs update [--check]

// Cada peça que conversa com o sistema tem um arquivo por plataforma, com a
// mesma cara: o Linux fica como sempre foi, e o Windows soma ao lado.
#[cfg_attr(windows, path = "audio_win.rs")]
mod audio;
#[cfg(windows)]
#[path = "hotkeys_win.rs"]
mod hotkeys;
#[cfg_attr(windows, path = "output_win.rs")]
mod output;
mod remote;
mod service;
mod stt;
#[cfg(unix)]
mod focus;
#[cfg(unix)]
mod speech;
#[cfg(unix)]
mod t3;
#[cfg_attr(windows, path = "tray_win.rs")]
mod tray;
mod update;
mod worker;

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "talk-rs", about = "Ditado por voz local: atalho, Whisper na GPU, texto no cursor")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Sobe o serviço.
    Serve,
    /// Só no Windows: sobe o serviço em segundo plano, sem janela, com o log
    /// em `%LOCALAPPDATA%\talk\serve.log`.
    #[cfg(windows)]
    Start,
    /// Só no Windows: sobe (on) ou não (off) no login.
    #[cfg(windows)]
    Autostart {
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// Começa a ditada; chamado de novo, termina e cola.
    Toggle {
        /// pt, en ou auto. Sem isto, vale o `language` do config.
        #[arg(short, long)]
        language: Option<String>,
    },
    /// Fala o texto (ou o stdin) e volta na hora; as falas fazem fila.
    #[cfg(unix)]
    Speak {
        /// O título da conversa do T3 de onde vem a fala. Sem isto, o do
        /// `T3CODE_THREAD_ID` (quem repassa de outra máquina manda o de lá).
        #[arg(long)]
        thread: Option<String>,
        /// Sem isto, lê do stdin.
        text: Vec<String>,
    },
    /// Cala a fala de agora e esvazia a fila.
    #[cfg(unix)]
    Hush,
    /// Fala de novo a última fala (o mesmo que clicar no ícone da bandeja),
    /// ou uma do histórico pelo id (`talk-rs again 20261124-143210-123`).
    #[cfg(unix)]
    Again { id: Option<String> },
    /// Em que fase o serviço está.
    Status,
    /// Atualiza para a última versão publicada; o talk reinicia sozinho.
    Update {
        /// Só procura e diz se há versão nova.
        #[arg(long)]
        check: bool,
    },
    /// Só no Windows: registra de novo os atalhos do `[hotkeys]` do config,
    /// depois de editá-lo à mão (a janela já faz isso sozinha).
    #[cfg(windows)]
    Hotkeys,
    /// Interno: o processo que transcreve.
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        model: PathBuf,
    },
}

fn main() -> Result<()> {
    // A versão com o commit: duas máquinas na mesma versão podem rodar códigos diferentes.
    let matches = Cli::command().version(&*String::leak(talk_core::update::version_line())).get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    match cli.cmd {
        Cmd::Update { check } => run_update(check),
        Cmd::Serve => service::serve(),
        #[cfg(windows)]
        Cmd::Start => service::start_detached(),
        #[cfg(windows)]
        Cmd::Autostart { state } => service::autostart(state == "on"),
        Cmd::Worker { model } => worker::run(&model),
        Cmd::Toggle { language } => {
            let reply = service::request(&service::Request { language, ..service::Request::new("toggle") })?;
            if let Some(e) = reply.error {
                bail!("{e}");
            }
            println!("{}", reply.phase);
            Ok(())
        }
        #[cfg(windows)]
        Cmd::Hotkeys => {
            let reply = service::request(&service::Request::new("hotkeys"))?;
            if let Some(e) = reply.error {
                bail!("{e}");
            }
            println!("atalhos registrados");
            Ok(())
        }
        #[cfg(unix)]
        Cmd::Speak { thread, text } => {
            let text = if text.is_empty() {
                std::io::read_to_string(std::io::stdin())?
            } else {
                text.join(" ")
            };
            let reply = service::request(&service::Request {
                text: Some(text),
                // Chamado de dentro de uma conversa do T3: a fala espera a janela dela.
                thread: thread.filter(|t| !t.trim().is_empty()).or_else(t3::current_thread_title),
                ..service::Request::new("speak")
            })?;
            if let Some(e) = reply.error {
                bail!("{e}");
            }
            Ok(())
        }
        #[cfg(unix)]
        Cmd::Again { id } => {
            if !service::request(&service::Request { text: id, ..service::Request::new("again") })?.ok {
                bail!("não achei essa fala para repetir");
            }
            Ok(())
        }
        #[cfg(unix)]
        Cmd::Hush => {
            service::request(&service::Request::new("hush"))?;
            Ok(())
        }
        Cmd::Status => {
            let reply = service::request(&service::Request::new("status"))?;
            let mut line = reply.phase.clone();
            if reply.speaking == Some(true) {
                line.push_str(" (falando)");
            }
            if let Some(n @ 1..) = reply.speech_waiting {
                line.push_str(&format!(" ({n} fala(s) esperando o foco)"));
            }
            println!("{line}");
            Ok(())
        }
    }
}

fn run_update(check: bool) -> Result<()> {
    use talk_core::update::VERSION;
    let ask = |cmd: &str| service::request(&service::Request::new(cmd));
    if check {
        let state = update::check()?;
        let _ = ask("update-refresh");
        match state.available() {
            Some(v) => println!("{v} disponível (esta é a {VERSION}): talk-rs update"),
            None => println!("em dia: {}", talk_core::update::version_line()),
        }
        return Ok(());
    }
    // Com o serviço de pé, ele mesmo atualiza (e avisa no fim); sem, aqui.
    if ask("update").is_ok_and(|r| r.ok) {
        println!("atualizando em segundo plano; o talk avisa e reinicia sozinho");
        return Ok(());
    }
    println!("{}", update::apply()?);
    Ok(())
}
