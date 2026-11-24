//! O motor local do app: `Engine` no Kotlin. Um modelo carregado por vez,
//! guardado aqui; o Kotlin só chama `load`, `transcribe` e `unload`.
//!
//! As funções `Java_…` precisam de `#[unsafe(no_mangle)]` para a JVM achá-las
//! pelo nome. É o único `unsafe` deste crate, e não há ponteiro cru nenhum: o
//! estado fica num `Mutex` daqui, não num `long` passado ao Kotlin.
#![allow(unsafe_code)]

// O nome do VAD fica no Kotlin (`Prefs.VAD`), que é quem baixa o arquivo.
#[allow(dead_code)]
#[path = "../../../crates/talk/src/stt.rs"]
mod stt;

use std::sync::Mutex;
use std::time::Instant;

use anyhow::{Context, Result};
use jni::JNIEnv;
use jni::objects::{JClass, JFloatArray, JString};
use jni::sys::{jboolean, jstring};
use serde_json::json;
use talk_core::vocab::Vocab;
use talk_core::wire::Job;
use whisper_rs::{WhisperContext, WhisperContextParameters, WhisperState};

struct Loaded {
    path: String,
    // O estado empresta do contexto em memória, mas o whisper-rs guarda um Arc:
    // os dois vivem juntos aqui.
    _ctx: WhisperContext,
    state: WhisperState,
}

static ENGINE: Mutex<Option<Loaded>> = Mutex::new(None);

fn engine() -> std::sync::MutexGuard<'static, Option<Loaded>> {
    ENGINE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn load(path: &str) -> Result<()> {
    let mut slot = engine();
    if slot.as_ref().is_some_and(|l| l.path == path) {
        return Ok(());
    }
    *slot = None; // o antigo sai antes: dois modelos grandes não cabem juntos
    let mut params = WhisperContextParameters::default();
    params.use_gpu(false).flash_attn(true);
    let ctx = WhisperContext::new_with_params(path, params).with_context(|| format!("não deu para carregar {path}"))?;
    let state = ctx.create_state()?;
    *slot = Some(Loaded { path: path.to_owned(), _ctx: ctx, state });
    Ok(())
}

fn transcribe(audio: &[f32], language: &str, vocab_txt: &str, vocab: &str, vad: &str) -> Result<serde_json::Value> {
    let started = Instant::now();
    let mut slot = engine();
    let loaded = slot.as_mut().context("nenhum modelo carregado")?;
    let words = Vocab::new(vocab_txt, vocab);
    let job = Job {
        language: if language == "auto" { String::new() } else { language.to_owned() },
        prompt: words.prompt(),
        // Na CPU do celular, busca em feixe custa caro demais para ditado.
        beam_size: 1,
        samples: audio.len(),
        fit_audio: true,
    };
    let vad = (!vad.is_empty() && std::path::Path::new(vad).exists()).then_some(vad);
    let (text, language) = stt::transcribe(&mut loaded.state, vad, &job, audio)?;
    Ok(json!({
        "text": words.apply(&text),
        "language": language,
        "stt_seconds": started.elapsed().as_secs_f64(),
    }))
}

fn string(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(Into::into).unwrap_or_default()
}

fn reply(env: &mut JNIEnv, v: &serde_json::Value) -> jstring {
    env.new_string(v.to_string()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// `Engine.load(path)`: `true` quando o modelo está pronto.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_fioravante_talk_Engine_load(mut env: JNIEnv, _: JClass, path: JString) -> jboolean {
    let path = string(&mut env, &path);
    match load(&path) {
        Ok(()) => 1,
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalStateException", format!("{e:#}"));
            0
        }
    }
}

/// `Engine.transcribe(audio, …)`: JSON com `text`, `language`, `stt_seconds`,
/// ou `error`. O áudio já vem mono, a 16 kHz.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_fioravante_talk_Engine_transcribe(
    mut env: JNIEnv,
    _: JClass,
    audio: JFloatArray,
    language: JString,
    vocab_txt: JString,
    vocab: JString,
    vad: JString,
) -> jstring {
    let len = env.get_array_length(&audio).unwrap_or(0).max(0) as usize;
    let mut samples = vec![0f32; len];
    if env.get_float_array_region(&audio, 0, &mut samples).is_err() {
        return reply(&mut env, &json!({"error": "não deu para ler o áudio"}));
    }
    let (language, vocab_txt, vocab, vad) =
        (string(&mut env, &language), string(&mut env, &vocab_txt), string(&mut env, &vocab), string(&mut env, &vad));
    let out = transcribe(&samples, &language, &vocab_txt, &vocab, &vad)
        .unwrap_or_else(|e| json!({"error": format!("{e:#}")}));
    reply(&mut env, &out)
}

/// `Engine.unload()`: devolve a memória (o modelo grande ocupa ~1 GB de RAM).
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_fioravante_talk_Engine_unload(_: JNIEnv, _: JClass) {
    *engine() = None;
}
