//! `talk-start.exe`: o que o login do Windows chama. É um app sem console
//! (`windows_subsystem`), então nada pisca na tela; ele só pede ao `talk-rs`
//! ao lado que suba o serviço sem janela, e sai.
//!
//! No Linux não tem função: lá o autostart chama `talk-rs serve` direto.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe.with_file_name("talk-rs.exe"))
                .arg("start")
                .creation_flags(NO_WINDOW)
                .status();
        }
    }
    #[cfg(not(windows))]
    eprintln!("talk-start é só do Windows; no Linux, talk-rs serve");
}
