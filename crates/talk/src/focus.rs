//! Qual janela do T3 está em foco, pelo `zcosmic_toplevel_info_v1` do COSMIC.
//!
//! O título da janela do T3 é "Projeto / Conversa": com ele a fala sabe se a
//! pessoa está olhando a conversa que falou. Fora do COSMIC (sem o protocolo)
//! o vigia não sobe, e quem perguntar ouve `None` para sempre.
//!
//! Também diz se a pessoa largou o PC (30 s sem teclado nem mouse, pelo
//! `ext_idle_notify_v1`): com dois PCs, a fala vai para o que está em uso.

use std::collections::HashMap;
use std::thread;

use anyhow::{Context, Result, anyhow};
use cosmic_protocols::toplevel_info::v1::client::{zcosmic_toplevel_handle_v1, zcosmic_toplevel_info_v1};
use wayland_client::backend::ObjectId;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_protocols::ext::idle_notify::v1::client::{ext_idle_notification_v1, ext_idle_notifier_v1};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};

/// O app id do T3 Code (o AppImage).
const T3_APP_ID: &str = "com.t3tools.T3Code";
/// O valor de "activated" no `state` do protocolo.
const ACTIVATED: u32 = 2;

/// Sem teclado nem mouse por isto, a pessoa largou o PC.
pub const IDLE_MS: u32 = 30_000;

/// O que o vigia viu.
pub enum Seen<'a> {
    /// O título da janela do T3 em foco, ou `None` quando o foco sai do T3.
    Focus(Option<&'a str>),
    /// `true`: ninguém mexe no PC há `IDLE_MS`; `false`: voltou.
    Idle(bool),
}

pub type OnChange = Box<dyn Fn(Seen) + Send>;

#[derive(Default)]
struct Window {
    title: String,
    app_id: String,
    active: bool,
}

struct State {
    info: Option<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1>,
    seat: Option<wl_seat::WlSeat>,
    idle: Option<ext_idle_notifier_v1::ExtIdleNotifierV1>,
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
            (self.on_change)(Seen::Focus(now.as_deref()));
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
    let mut state = State { info: None, seat: None, idle: None, windows: HashMap::new(), focused: None, on_change };
    queue.roundtrip(&mut state)?;
    if state.info.is_none() {
        return Err(anyhow!("o compositor não tem zcosmic_toplevel_info_v1 (não é o COSMIC)"));
    }
    match (&state.idle, &state.seat) {
        (Some(n), Some(seat)) if n.version() >= 2 => drop(n.get_input_idle_notification(IDLE_MS, seat, &qh, ())),
        (Some(n), Some(seat)) => drop(n.get_idle_notification(IDLE_MS, seat, &qh, ())),
        _ => eprintln!("[focus] sem ext_idle_notify_v1: não sei quando o PC está largado"),
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
        let wl_registry::Event::Global { name, interface, version } = event else { return };
        match interface.as_str() {
            // A versão 1: as janelas chegam pelo evento `toplevel`, sem
            // precisar da lista do ext-foreign-toplevel.
            "zcosmic_toplevel_info_v1" => state.info = Some(registry.bind(name, 1, qh, ())),
            "wl_seat" if state.seat.is_none() => state.seat = Some(registry.bind(name, 1, qh, ())),
            // A versão 2 tem o aviso que ignora quem segura a tela acesa (um
            // vídeo, um navegador): o que importa é teclado e mouse.
            "ext_idle_notifier_v1" => state.idle = Some(registry.bind(name, version.min(2), qh, ())),
            _ => {}
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

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ext_idle_notifier_v1::ExtIdleNotifierV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ext_idle_notifier_v1::ExtIdleNotifierV1,
        _: ext_idle_notifier_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ext_idle_notification_v1::ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => (state.on_change)(Seen::Idle(true)),
            ext_idle_notification_v1::Event::Resumed => (state.on_change)(Seen::Idle(false)),
            _ => {}
        }
    }
}
