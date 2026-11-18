//! Os atalhos no Windows. Lá não há atalho do sistema que rode um comando
//! (no Linux o COSMIC chama `talk-rs toggle`), então o próprio serviço
//! registra as teclas, uma por idioma, a partir do `[hotkeys]` do config.
//!
//! O `RegisterHotKey` só entrega a tecla na thread que a registrou: tudo aqui
//! vive numa thread só, presa no laço de mensagens. Para trocar as teclas (a
//! janela mudou o config), `reload` interrompe o laço; a thread solta todas,
//! registra as do config de novo e responde o que não deu para registrar.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use talk_core::config::Config;
use windows_hotkeys::keys::{ModKey, VKey};
use windows_hotkeys::singlethreaded::HotkeyManager;
use windows_hotkeys::{HotkeyManagerImpl, InterruptHandle};

use crate::service::Service;

/// O canal até a thread dos atalhos: cada pedido leva onde responder.
struct Control {
    interrupt: InterruptHandle,
    requests: Sender<Sender<Vec<String>>>,
}

static CONTROL: OnceLock<Mutex<Control>> = OnceLock::new();

/// "ctrl+alt+d" → as modificadoras e a tecla. `NoRepeat`: segurar a tecla não
/// liga e desliga a gravação em rajada.
fn parse(combo: &str) -> Result<(VKey, Vec<ModKey>)> {
    let parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last().ok_or_else(|| anyhow!("atalho vazio"))?;
    let mut mods = mods
        .iter()
        .map(|m| ModKey::from_keyname(m).map_err(|e| anyhow!("{combo:?}: {e}")))
        .collect::<Result<Vec<_>>>()?;
    mods.push(ModKey::NoRepeat);
    let key = VKey::from_keyname(key).map_err(|e| anyhow!("{combo:?}: {e}"))?;
    Ok((key, mods))
}

/// Solta tudo e registra o `[hotkeys]` de agora. Devolve um erro por atalho
/// que não entrou (outro programa já usa a tecla, ou o Windows a reserva).
fn register_all(hkm: &mut HotkeyManager<()>, service: &Arc<Service>) -> Vec<String> {
    let _ = hkm.unregister_all();
    let cfg = Config::load(&talk_core::paths::config_file()).unwrap_or_default();
    let mut errors = vec![];
    for (combo, language) in cfg.hotkeys {
        let me = service.clone();
        let registered = parse(&combo).and_then(|(key, mods)| {
            let combo_now = combo.clone();
            hkm.register(key, &mods, move || {
                // O idioma é relido a cada aperto: trocar na janela vale na hora.
                let language = Config::load(&talk_core::paths::config_file())
                    .ok()
                    .and_then(|c| c.hotkeys.get(&combo_now).cloned())
                    .unwrap_or_else(|| language.clone());
                eprintln!("[talk] atalho apertado ({language})");
                if let Err(e) = me.toggle(Some(language)) {
                    eprintln!("[talk] {e:#}");
                }
            })
            .map_err(|e| anyhow!("{e}"))
            .context("outro programa já usa estas teclas, ou o Windows as reserva")
        });
        match registered {
            Ok(_) => eprintln!("[talk] atalho {combo}"),
            Err(e) => {
                eprintln!("[talk] sem o atalho {combo}: {e:#}");
                // Só a nossa explicação: a da crate vem em inglês.
                errors.push(format!("{}: {e}", talk_core::config::hotkey_keys(&combo).join("+")));
            }
        }
    }
    errors
}

pub fn spawn(service: Arc<Service>) {
    std::thread::spawn(move || {
        let mut hkm = HotkeyManager::<()>::new();
        let (tx, rx): (_, Receiver<Sender<Vec<String>>>) = mpsc::channel();
        let _ = CONTROL.set(Mutex::new(Control { interrupt: hkm.interrupt_handle(), requests: tx }));
        loop {
            let errors = register_all(&mut hkm, &service);
            while let Ok(reply) = rx.try_recv() {
                let _ = reply.send(errors.clone());
            }
            // Volta daqui quando `reload` interrompe.
            hkm.event_loop();
        }
    });
}

/// Registra as teclas do config de novo. `Err` com o que não deu certo.
pub fn reload() -> Result<()> {
    let control = CONTROL.get().context("os atalhos ainda não subiram")?;
    let (tx, rx) = mpsc::channel();
    {
        let c = control.lock().unwrap_or_else(PoisonError::into_inner);
        c.requests.send(tx).map_err(|_| anyhow!("a thread dos atalhos parou"))?;
        c.interrupt.interrupt();
    }
    let errors = rx.recv_timeout(Duration::from_secs(3)).context("os atalhos não responderam")?;
    if errors.is_empty() { Ok(()) } else { Err(anyhow!(errors.join("; "))) }
}
