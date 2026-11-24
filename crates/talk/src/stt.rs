//! Áudio → texto com o whisper.cpp: o VAD corta o silêncio, o Whisper
//! transcreve, e o que é alucinação conhecida vira vazio.
//!
//! Este arquivo é compartilhado: o trabalhador do PC o usa daqui, e o app
//! Android (`android/rust`) o inclui por `#[path]`, para os dois transcreverem
//! do mesmo jeito. Por isso ele só depende do `whisper-rs` e do `talk-core`.

use anyhow::Result;
use talk_core::clean::{is_degenerate, is_known_hallucination, keep_speech};
use talk_core::wire::Job;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperVadContext, WhisperVadContextParams, WhisperVadParams, get_lang_str,
};

const RATE: usize = talk_core::wav::RATE as usize;

pub const VAD_MODEL: &str = "ggml-silero-v5.1.2.bin";

pub fn transcribe(
    state: &mut whisper_rs::WhisperState,
    vad: Option<&str>,
    job: &Job,
    audio: &[f32],
) -> Result<(String, String)> {
    // Menos de 0,3 s não é uma frase; o Whisper inventaria uma.
    if audio.len() < RATE * 3 / 10 {
        return Ok((String::new(), String::new()));
    }
    let mut full = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: job.beam_size.max(1),
        patience: -1.0,
    });
    full.set_language(Some(if job.language.is_empty() { "auto" } else { &job.language }));
    if !job.prompt.is_empty() {
        full.set_initial_prompt(&job.prompt);
    }
    // Ditado são falas curtas e independentes: arrastar contexto entre
    // segmentos é a principal fonte de alucinação em laço.
    full.set_no_context(true);
    full.set_no_timestamps(true);
    full.set_suppress_blank(true);
    full.set_print_progress(false);
    full.set_print_realtime(false);
    full.set_print_special(false);
    let speech;
    let audio = match vad {
        Some(path) => {
            // Um contexto novo por ditada: reaproveitado, ele fica mais lento a
            // cada chamada (17 → 34 ms com o mesmo áudio). Criar custa ~1 ms.
            // E uma thread só: as threads do ggml esperam girando, e com a CPU
            // ocupada (uma compilação, por exemplo) 16 ms viraram 5,7 s.
            let mut cp = WhisperVadContextParams::new();
            cp.set_n_threads(1);
            let mut vad = WhisperVadContext::new(path, cp)?;
            let mut vp = WhisperVadParams::new();
            vp.set_min_silence_duration(500); // o mesmo corte do Python
            let spans: Vec<(f32, f32)> =
                vad.segments_from_samples(vp, audio)?.map(|s| (s.start, s.end)).collect();
            speech = keep_speech(audio, &spans, RATE / 5);
            if speech.is_empty() {
                return Ok((String::new(), String::new()));
            }
            &speech[..]
        }
        None => audio,
    };
    state.full(full, audio)?;

    let mut parts = vec![];
    for seg in state.as_iter() {
        // O mesmo corte do Python: o Whisper se autoavalia como "sem fala".
        if seg.no_speech_probability() >= 0.7 {
            continue;
        }
        parts.push(seg.to_str_lossy()?.trim().to_owned());
    }
    let text = parts.join(" ").trim().to_owned();
    let language = get_lang_str(state.full_lang_id_from_state()).unwrap_or("").to_owned();
    let junk = is_degenerate(&text) || is_known_hallucination(&text);
    Ok((if junk { String::new() } else { text }, language))
}

