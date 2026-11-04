//! `talk-rs`: o ditado do talk, em Rust.
//!
//!     talk-rs serve            o serviço (bandeja, atalhos, colagem)
//!     talk-rs toggle [-l pt]   o que o atalho chama: começa ou termina a ditada
//!     talk-rs status

mod audio;
mod output;
mod service;
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
