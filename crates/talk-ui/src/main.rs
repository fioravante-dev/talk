//! `talk-ui`: a janela do talk.
//!
//! Um webview (wry, o motor do Tauri) sem servidor HTTP: as páginas saem por um
//! protocolo interno, `talk://`, que só este processo atende. Nenhuma porta
//! TCP; um site aberto no navegador não alcança nada daqui.
//!
//! O estado ao vivo vem do serviço por uma conexão `subscribe` no socket dele e
//! entra na página por `evaluate_script`, no instante do evento.
//!
//! Uma janela só: abrir de novo traz a que já existe para a frente.

mod pages;
mod system;

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use talk_core::paths;
use wry::WebViewBuilder;
use wry::http::{Request, Response, header::CONTENT_TYPE};

#[derive(Debug)]
enum UserEvent {
    Script(String),
    Raise,
}

const CSS: &[u8] = include_bytes!("../assets/app.css");
const JS: &[u8] = include_bytes!("../assets/app.js");
const HTMX: &[u8] = include_bytes!("../assets/htmx.min.js");
const FONT: &[u8] = include_bytes!("../assets/schibsted-latin.woff2");

fn respond(status: u16, mime: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, mime)
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(b"".as_slice())))
}

fn asset(bytes: &'static [u8], mime: &str) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .header(CONTENT_TYPE, mime)
        .body(Cow::Borrowed(bytes))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(b"".as_slice())))
}

fn parse_form(text: &str) -> HashMap<String, String> {
    form_urlencoded::parse(text.as_bytes()).into_owned().collect()
}

fn route(req: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = req.uri().path();
    let query = parse_form(req.uri().query().unwrap_or(""));
    let form = parse_form(&String::from_utf8_lossy(req.body()));
    let post = req.method() == "POST";
    let page = match (post, path) {
        (_, "/assets/app.css") => return asset(CSS, "text/css"),
        (_, "/assets/app.js") => return asset(JS, "text/javascript"),
        (_, "/assets/htmx.min.js") => return asset(HTMX, "text/javascript"),
        (_, "/assets/schibsted-latin.woff2") => return asset(FONT, "font/woff2"),
        (false, "/ajustes") => pages::ajustes(),
        (false, "/" | "/historico") => pages::historico(query.get("q").map_or("", String::as_str), None),
        (false, "/vocabulario") => pages::vocabulario("", ""),
        (true, "/historico/copiar") => pages::copiar(&form),
        (true, p) if p.starts_with("/ajustes/") => pages::ajustar(&p["/ajustes/".len()..], &form),
        (true, p) if p.starts_with("/vocabulario/") => pages::vocab_action(&p["/vocabulario/".len()..], &form),
        _ => return respond(404, "text/plain; charset=utf-8", b"nada aqui".to_vec()),
    };
    match page {
        Ok(html) => respond(200, "text/html; charset=utf-8", html.into_bytes()),
        Err(e) => {
            eprintln!("[talk-ui] {path}: {e:#}");
            respond(500, "text/plain; charset=utf-8", format!("{e:#}").into_bytes())
        }
    }
}

/// Fica ouvindo o serviço; se ele cair, avisa a página e tenta de novo.
fn subscribe(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        loop {
            if let Ok(stream) = UnixStream::connect(paths::socket()) {
                let mut out = &stream;
                if writeln!(out, r#"{{"cmd":"subscribe"}}"#).is_ok() {
                    for line in BufReader::new(&stream).lines() {
                        let Ok(line) = line else { break };
                        if proxy.send_event(UserEvent::Script(line)).is_err() {
                            return; // a janela fechou
                        }
                    }
                }
            }
            if proxy.send_event(UserEvent::Script(r#"{"event":"phase","phase":"off"}"#.into())).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

fn ui_socket() -> std::path::PathBuf {
    paths::socket().with_file_name("talk-ui.sock")
}

/// `talk-ui --dump DIR`: escreve as telas como arquivos, para conferir o
/// visual num navegador headless sem abrir janela nenhuma.
fn dump(dir: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir.join("assets"))?;
    for (name, bytes) in [
        ("app.css", CSS),
        ("app.js", JS),
        ("htmx.min.js", HTMX),
        ("schibsted-latin.woff2", FONT),
    ] {
        std::fs::write(dir.join("assets").join(name), bytes)?;
    }
    for (name, html) in [
        ("ajustes", pages::ajustes()?),
        ("historico", pages::historico("", None)?),
        ("vocabulario", pages::vocabulario("", "")?),
    ] {
        std::fs::write(dir.join(format!("{name}.html")), html.replace("\"/assets/", "\"assets/").replace("(\"/assets/", "(\"assets/"))?;
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--dump") {
        return dump(std::path::Path::new(args.get(2).map_or("talk-ui-dump", String::as_str)));
    }
    // Já existe uma janela: ela vem para a frente, e este processo sai.
    if let Ok(mut s) = UnixStream::connect(ui_socket()) {
        let _ = writeln!(s, "raise");
        return Ok(());
    }
    let _ = std::fs::remove_file(ui_socket());
    let listener = UnixListener::bind(ui_socket()).ok();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("talk")
        .with_inner_size(LogicalSize::new(960.0, 680.0))
        .with_min_inner_size(LogicalSize::new(720.0, 520.0))
        .build(&event_loop)?;

    let builder = WebViewBuilder::new()
        .with_custom_protocol("talk".into(), move |_id, req| route(&req))
        .with_url("talk://localhost/historico")
        .with_background_color((0xe1, 0xe2, 0xe0, 0xff));
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or_else(|| anyhow::anyhow!("sem GTK vbox"))?;
        builder.build_gtk(vbox)?
    };

    subscribe(event_loop.create_proxy());
    if let Some(listener) = listener {
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || {
            for _ in listener.incoming() {
                let _ = proxy.send_event(UserEvent::Raise);
            }
        });
    }

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Script(json)) => {
                let _ = webview.evaluate_script(&format!("window.talkEvent && talkEvent({json})"));
            }
            Event::UserEvent(UserEvent::Raise) => {
                window.set_visible(true);
                window.set_focus();
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                let _ = std::fs::remove_file(ui_socket());
                *flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}
