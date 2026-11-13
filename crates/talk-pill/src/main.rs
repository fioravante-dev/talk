//! `talk-pill`: a pílula que aparece enquanto o talk grava.
//!
//! No Linux é GTK + layer-shell + cairo (`linux.rs`); no Windows, uma janela
//! transparente com o mesmo desenho em HTML (`main_win.rs`).

// No Windows, um app sem console: a pílula não pode piscar uma janela preta.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(unix)]
mod linux;
mod paint;
#[cfg(windows)]
mod main_win;

#[cfg(unix)]
fn main() {
    linux::main();
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    main_win::main()
}
