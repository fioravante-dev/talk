//! As ditadas que chegam pela rede: o app do celular (ou outro PC) grava,
//! manda o WAV, e aqui ele vira texto no mesmo trabalhador da GPU, com o mesmo
//! vocabulário e os mesmos filtros da ditada local.
//!
//! Liga com `listen = "<ip do tailscale>:7878"` no config. Sem TLS e sem
//! conta: quem protege é a tailnet (e o `listen_token`, se definido).
//!
//!     GET  /health                    {"ok":true,"host":"pop-os","version":"…","model":"…"}
//!     GET  /peers                     os talks da tailnet que responderam: [{"name","url"}]
//!     GET  /vocab                     {"vocab":"…","vocab_txt":"…"}: o app guarda para ditar sem o PC
//!     POST /warm                      sobe o trabalhador já (o app chama ao começar a gravar)
//!     POST /transcribe?language=pt    corpo: WAV → {"text","language","stt_seconds","audio_seconds"}
//!     GET  /speech/presence           {"speech": bool, "present_secs": n|null}: há quanto
//!                                     tempo a pessoa usou este PC com o T3 na frente
//!     POST /speech/speak              corpo: {"text", "thread"?} → fala aqui (o `talk-relay`)
//!     POST /speech/hush               cala
//!     POST /speech/again              fala de novo a última

use std::io::Read;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::json;
use talk_core::config::Config;
use talk_core::{history, paths, wav};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::service::Service;

/// Meia hora de WAV estéreo a 48 kHz cabe; mais que isso é engano.
const MAX_BODY: u64 = 400 * 1024 * 1024;

pub fn spawn(service: Arc<Service>) {
    let cfg = Config::load(&paths::config_file()).unwrap_or_default();
    let addr = cfg.listen.trim().to_owned();
    if addr.is_empty() {
        return;
    }
    thread::spawn(move || {
        // No login o Tailscale pode subir depois do talk, e o endereço ainda
        // não existe: tenta de novo até dar.
        let server = loop {
            match Server::http(&addr) {
                Ok(s) => break s,
                Err(e) => {
                    eprintln!("[remoto] sem {addr} ainda ({e}); tentando de novo em 10 s");
                    thread::sleep(Duration::from_secs(10));
                }
            }
        };
        eprintln!("[remoto] ouvindo em http://{addr}");
        for req in server.incoming_requests() {
            let me = service.clone();
            thread::spawn(move || handle(&me, req));
        }
    });
}

fn handle(service: &Service, mut req: Request) {
    let cfg = Config::load(&paths::config_file()).unwrap_or_default();
    let (path, query) = req.url().split_once('?').unwrap_or((req.url(), ""));
    let (path, query) = (path.to_owned(), query.to_owned());
    let reply = if !authorized(&req, &cfg.listen_token) {
        Err((401, "token ausente ou errado".to_owned()))
    } else {
        match (req.method(), path.as_str()) {
            (Method::Get, "/health") => Ok(json!({
                "ok": true,
                "host": hostname(),
                "version": talk_core::update::VERSION,
                "model": cfg.ggml_model,
            })),
            (Method::Get, "/peers") => Ok(json!(peers(&cfg))),
            (Method::Get, "/vocab") => Ok(json!({
                "vocab": cfg.vocab,
                "vocab_txt": std::fs::read_to_string(paths::vocab_file()).unwrap_or_default(),
            })),
            (Method::Post, "/warm") => {
                service.ensure_worker(&cfg).map(|()| json!({"ok": true})).map_err(|e| (500, format!("{e:#}")))
            }
            (Method::Post, "/transcribe") => {
                transcribe(service, &cfg, &mut req, param(&query, "language").as_deref()).map_err(|e| (400, format!("{e:#}")))
            }
            (Method::Get, "/speech/presence") => Ok(json!({
                "host": hostname(),
                // Ligada e instalada: só assim vale mandar a fala para cá.
                "speech": cfg.speech_enabled && crate::speech::installed(&cfg),
                "present_secs": service.speech.presence().map(|d| d.as_secs()),
            })),
            (Method::Post, "/speech/speak") => speak(service, &mut req).map_err(|e| (400, format!("{e:#}"))),
            (Method::Post, "/speech/hush") => {
                service.speech.hush();
                Ok(json!({"ok": true}))
            }
            (Method::Post, "/speech/again") => Ok(json!({"ok": service.speech.replay()})),
            _ => Err((404, format!("{path}: não existe"))),
        }
    };
    let (code, body) = match reply {
        Ok(v) => (200, v),
        Err((code, msg)) => {
            eprintln!("[remoto] {code} {path}: {msg}");
            (code, json!({"error": msg}))
        }
    };
    let mut resp = Response::from_string(body.to_string()).with_status_code(code);
    if let Ok(h) = "Content-Type: application/json; charset=utf-8".parse::<Header>() {
        resp = resp.with_header(h);
    }
    let _ = req.respond(resp);
}

fn speak(service: &Service, req: &mut Request) -> Result<serde_json::Value> {
    #[derive(serde::Deserialize)]
    struct Body {
        text: String,
        #[serde(default)]
        thread: Option<String>,
    }
    let mut raw = String::new();
    req.as_reader().take(1024 * 1024).read_to_string(&mut raw)?;
    let body: Body = serde_json::from_str(&raw).context("esperava {\"text\": ...}")?;
    if body.text.trim().is_empty() {
        bail!("nada para falar");
    }
    service.speak(body.text, body.thread);
    Ok(json!({"ok": true}))
}

fn transcribe(service: &Service, cfg: &Config, req: &mut Request, language: Option<&str>) -> Result<serde_json::Value> {
    let started = Instant::now();
    let mut body = vec![];
    req.as_reader().take(MAX_BODY + 1).read_to_end(&mut body).context("lendo o áudio")?;
    if body.len() as u64 > MAX_BODY {
        bail!("áudio grande demais");
    }
    let audio = wav::decode(&body)?;
    let audio_seconds = audio.len() as f64 / f64::from(wav::RATE);
    let (text, done) = service.transcribe(cfg, &audio, language)?;
    if !text.is_empty() {
        history::append(
            &paths::history_file(),
            &history::Line {
                at: history::now(),
                text: &text,
                language: &done.language,
                motor: "whisper.cpp",
                delivered: &["remote"],
                audio_seconds,
                stt_seconds: done.stt_seconds,
            },
        )?;
    }
    eprintln!(
        "[remoto] {audio_seconds:.1}s de áudio, STT {:.2}s, pedido {:.2}s",
        done.stt_seconds,
        started.elapsed().as_secs_f64()
    );
    Ok(json!({
        "text": text,
        "language": done.language,
        "stt_seconds": done.stt_seconds,
        "audio_seconds": audio_seconds,
    }))
}

fn hostname() -> String {
    let env = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default();
    if !env.is_empty() {
        return env;
    }
    std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_owned()).unwrap_or_default()
}

/// Os talks da tailnet: pergunta ao `tailscale status` quem está online e
/// bate no `/health` de cada um, na mesma porta deste, todos ao mesmo tempo.
/// O app do celular usa isto para achar os PCs sozinho.
fn peers(cfg: &Config) -> Vec<serde_json::Value> {
    let port = cfg.listen.rsplit_once(':').map_or("7878", |(_, p)| p).to_owned();
    let Ok(out) = std::process::Command::new("tailscale").args(["status", "--json"]).output() else {
        return vec![];
    };
    let Ok(status) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return vec![] };
    let mut nodes = vec![];
    let mut add = |n: &serde_json::Value| {
        let ip = n["TailscaleIPs"].as_array().and_then(|ips| ips.iter().filter_map(|i| i.as_str()).find(|i| !i.contains(':')));
        if let (Some(ip), Some(name)) = (ip, n["HostName"].as_str()) {
            nodes.push((name.to_owned(), format!("{ip}:{port}")));
        }
    };
    add(&status["Self"]);
    if let Some(peers) = status["Peer"].as_object() {
        for p in peers.values().filter(|p| p["Online"].as_bool() == Some(true)) {
            add(p);
        }
    }
    let token = cfg.listen_token.clone();
    let probes: Vec<_> = nodes
        .into_iter()
        .map(|(name, addr)| {
            let token = token.clone();
            thread::spawn(move || answers(&addr, &token).then(|| json!({"name": name, "url": format!("http://{addr}")})))
        })
        .collect();
    probes.into_iter().filter_map(|t| t.join().ok().flatten()).collect()
}

/// Um `GET /health` à mão, com prazo curto: não vale uma biblioteca de HTTP.
fn answers(addr: &str, token: &str) -> bool {
    use std::io::Write;
    use std::net::{TcpStream, ToSocketAddrs};
    let Some(sock) = addr.to_socket_addrs().ok().and_then(|mut a| a.next()) else { return false };
    let Ok(mut s) = TcpStream::connect_timeout(&sock, Duration::from_millis(800)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
    let auth = if token.is_empty() { String::new() } else { format!("Authorization: Bearer {token}\r\n") };
    if write!(s, "GET /health HTTP/1.0\r\nHost: {addr}\r\n{auth}\r\n").is_err() {
        return false;
    }
    let mut body = String::new();
    let _ = s.read_to_string(&mut body);
    body.contains("\"ok\":true")
}

fn authorized(req: &Request, token: &str) -> bool {
    if token.is_empty() {
        return true;
    }
    let want = format!("Bearer {token}");
    req.headers().iter().any(|h| h.field.equiv("Authorization") && h.value.as_str() == want)
}

/// Um parâmetro da query, sem decodificar `%`: os valores são `pt`, `en`, `auto`.
fn param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_comes_from_the_query() {
        assert_eq!(param("language=pt", "language").as_deref(), Some("pt"));
        assert_eq!(param("x=1&language=auto", "language").as_deref(), Some("auto"));
        assert_eq!(param("", "language"), None);
    }
}
