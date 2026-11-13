//! `talk-start.exe`: o que o login e o menu Iniciar do Windows chamam. É um
//! app sem console (`windows_subsystem`), então nada pisca na tela; ele pede ao
//! `talk-rs` ao lado que suba o serviço sem janela, e sai.
//!
//! `talk-start --open` (o atalho do menu Iniciar) também mostra a janela: quem
//! procura o talk no menu quer ver alguma coisa.
//!
//! No Linux não tem função: lá o autostart chama `talk-rs serve` direto.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::time::Duration;
        const NO_WINDOW: u32 = 0x0800_0000;
        let Ok(exe) = std::env::current_exe() else { return };
        // Se o serviço já roda, o `start` só avisa e sai: tudo bem.
        let _ = std::process::Command::new(exe.with_file_name("talk-rs.exe"))
            .arg("start")
            .creation_flags(NO_WINDOW)
            .status();
        if std::env::args().any(|a| a == "--open") {
            // O serviço sobe a janela escondida; espera ela atender para só
            // trazê-la para a frente, em vez de abrir uma segunda.
            let ui = talk_core::paths::socket().with_file_name("talk-ui.sock");
            for _ in 0..50 {
                if uds_windows::UnixStream::connect(&ui).is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = std::process::Command::new(exe.with_file_name("talk-ui.exe")).spawn();
        }
    }
    #[cfg(not(windows))]
    eprintln!("talk-start é só do Windows; no Linux, talk-rs serve");
}
