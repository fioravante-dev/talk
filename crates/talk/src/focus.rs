//! Qual janela do T3 está em foco, pelo `zcosmic_toplevel_info_v1` do COSMIC.
//!
//! O título da janela do T3 é "Projeto / Conversa": com ele a fala sabe se a
//! pessoa está olhando a conversa que falou. Fora do COSMIC (sem o protocolo)
//! o vigia não sobe, e quem perguntar ouve `None` para sempre.

use std::collections::HashMap;
use std::thread;

use anyhow::{Context, Result, anyhow};
use cosmic_protocols::toplevel_info::v1::client::{zcosmic_toplevel_handle_v1, zcosmic_toplevel_info_v1};
use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};

/// O app id do T3 Code (o AppImage).
const T3_APP_ID: &str = "com.t3tools.T3Code";
/// O valor de "activated" no `state` do protocolo.
const ACTIVATED: u32 = 2;

/// Quem quer saber quando o foco muda: recebe o título da janela do T3 em
/// foco, ou `None` quando o foco sai do T3.
pub type OnChange = Box<dyn Fn(Option<&str>) + Send>;

#[derive(Default)]
struct Window {
    title: String,
    app_id: String,
    active: bool,
}

struct State {
    info: Option<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1>,
    windows: HashMap<ObjectId, Window>,
    focused: Option<String>,
    on_change: OnChange,
}

impl State {
    /// Depois de cada lote de eventos: o T3 em foco mudou?
    fn settle(&mut self) {
        let now = self
            .windows
            .values()
            .find(|w| w.active && w.app_id == T3_APP_ID)
            .map(|w| w.title.clone());
        if self.focused != now {
            (self.on_change)(now.as_deref());
            self.focused = now;
        }
    }
}

/// Sobe o vigia numa thread; `on_change` já é chamado com o foco de agora.
/// Erro se não há COSMIC para perguntar (o serviço subiu fora da sessão).
pub fn watch(on_change: OnChange) -> Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err(anyhow!("sem WAYLAND_DISPLAY"));
    }
    let conn = Connection::connect_to_env().context("sem conexão com o Wayland")?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    let mut state = State { info: None, windows: HashMap::new(), focused: None, on_change };
    queue.roundtrip(&mut state)?;
    if state.info.is_none() {
        return Err(anyhow!("o compositor não tem zcosmic_toplevel_info_v1 (não é o COSMIC)"));
    }
    queue.roundtrip(&mut state)?;
    thread::spawn(move || {
        loop {
            if let Err(e) = queue.blocking_dispatch(&mut state) {
                eprintln!("[focus] o Wayland caiu, o vigia para: {e}");
                return;
            }
        }
    });
    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, .. } = event
            && interface == "zcosmic_toplevel_info_v1"
        {
            // A versão 1: as janelas chegam pelo evento `toplevel`, sem
            // precisar da lista do ext-foreign-toplevel.
            state.info = Some(registry.bind(name, 1, qh, ()));
        }
    }
}

impl Dispatch<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
        event: zcosmic_toplevel_info_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zcosmic_toplevel_info_v1::Event::Toplevel { toplevel } = event {
            state.windows.insert(toplevel.id(), Window::default());
        }
    }

    event_created_child!(State, zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, [
        zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, ()),
    ]);
}

impl Dispatch<zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zcosmic_toplevel_handle_v1::Event;
        let id = handle.id();
        match event {
            Event::Title { title } => state.windows.entry(id).or_default().title = title,
            Event::AppId { app_id } => state.windows.entry(id).or_default().app_id = app_id,
            Event::State { state: raw } => {
                state.windows.entry(id).or_default().active = raw
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|c| u32::from_ne_bytes(*c) == ACTIVATED);
            }
            Event::Closed => {
                state.windows.remove(&id);
                handle.destroy();
                state.settle();
            }
            Event::Done => state.settle(),
            _ => {}
        }
    }
}
