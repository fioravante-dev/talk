//! As ditadas que chegam pela rede: o app do celular (ou outro PC) grava,
//! manda o WAV, e aqui ele vira texto no mesmo trabalhador da GPU, com o mesmo
//! vocabulário e os mesmos filtros da ditada local.
//!
//! Liga com `listen = "<ip do tailscale>:7878"` no config. Sem TLS e sem
//! conta: quem protege é a tailnet (e o `listen_token`, se definido).
//!
//!     GET  /health                    {"ok":true,"version":"…","model":"…"}
//!     GET  /vocab                     {"vocab":"…","vocab_txt":"…"}: o app guarda para ditar sem o PC
//!     POST /warm                      sobe o trabalhador já (o app chama ao começar a gravar)
//!     POST /transcribe?language=pt    corpo: WAV → {"text","language","stt_seconds","audio_seconds"}

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
                "version": talk_core::update::VERSION,
                "model": cfg.ggml_model,
            })),
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
