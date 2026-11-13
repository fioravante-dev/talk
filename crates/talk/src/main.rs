//! `talk-rs`: o ditado do talk, em Rust.
//!
//!     talk-rs serve            o serviço (bandeja, atalhos, colagem)
//!     talk-rs toggle [-l pt]   o que o atalho chama: começa ou termina a ditada
//!     talk-rs status

// Cada peça que conversa com o sistema tem um arquivo por plataforma, com a
// mesma cara: o Linux fica como sempre foi, e o Windows soma ao lado.
#[cfg_attr(windows, path = "audio_win.rs")]
mod audio;
#[cfg(windows)]
#[path = "hotkeys_win.rs"]
mod hotkeys;
#[cfg_attr(windows, path = "output_win.rs")]
mod output;
mod service;
#[cfg_attr(windows, path = "tray_win.rs")]
mod tray;
mod worker;

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

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
    /// Em que fase o serviço está.
    Status,
    /// Interno: o processo que transcreve.
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        model: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Serve => service::serve(),
        #[cfg(windows)]
        Cmd::Start => service::start_detached(),
        #[cfg(windows)]
        Cmd::Autostart { state } => service::autostart(state == "on"),
        Cmd::Worker { model } => worker::run(&model),
        Cmd::Toggle { language } => {
            let reply = service::request(&service::Request { cmd: "toggle".into(), language })?;
            if let Some(e) = reply.error {
                bail!("{e}");
            }
            println!("{}", reply.phase);
            Ok(())
        }
        Cmd::Status => {
            let reply = service::request(&service::Request { cmd: "status".into(), language: None })?;
            println!("{}", reply.phase);
            Ok(())
        }
    }
}
