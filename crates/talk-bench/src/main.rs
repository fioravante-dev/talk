//! Fase 0: o whisper.cpp transcreve os WAVs do corpus e diz quanto demorou.
//!
//! Um processo por rodada, de propósito: a carga a frio (processo novo → CUDA →
//! modelo na VRAM) é o que o trabalhador filho do plano paga depois de ocioso,
//! então ela é medida junto. A VRAM é medida por fora, por quem chama
//! (`tools/fase0.py`), porque o pico é do processo inteiro e não do modelo.
//!
//! Saída: uma linha JSON por arquivo e uma última com o resumo.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde_json::json;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, get_lang_str,
};

#[derive(Parser, Debug)]
struct Args {
    /// O modelo ggml (ex.: ~/.cache/talk/models/ggml-large-v3.bin).
    #[arg(long)]
    model: PathBuf,
    /// "pt", "en" ou vazio para detectar, como o `language` do config.
    #[arg(long, default_value = "")]
    language: String,
    /// A dica de vocabulário (o `initial_prompt` do Whisper).
    #[arg(long, default_value = "")]
    prompt: String,
    #[arg(long, default_value_t = 5)]
    beam_size: i32,
    /// Os WAVs, 16 kHz mono 16 bits, como o talk grava.
    wavs: Vec<PathBuf>,
}

fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path).with_context(|| path.display().to_string())?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 || spec.bits_per_sample != 16 {
        bail!("{}: precisa ser 16 kHz mono 16 bits, veio {spec:?}", path.display());
    }
    reader
        .samples::<i16>()
        .map(|s| Ok(f32::from(s?) / 32_768.0))
        .collect()
}

fn main() -> Result<()> {
    let args = Args::parse();
    let started = Instant::now();

    let mut params = WhisperContextParameters::default();
    params.use_gpu(true).flash_attn(true);
    let model = args.model.to_str().context("caminho do modelo não é UTF-8")?;
    let ctx = WhisperContext::new_with_params(model, params)?;
    let mut state = ctx.create_state()?;
    let load_s = started.elapsed().as_secs_f64();

    let mut stt_total = 0.0;
    let mut audio_total = 0.0;
    let mut first_text_s = None;
    for wav in &args.wavs {
        let audio = read_wav(wav)?;
        let audio_s = audio.len() as f64 / 16_000.0;

        let mut full = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: args.beam_size,
            patience: -1.0,
        });
        full.set_language(if args.language.is_empty() { Some("auto") } else { Some(&args.language) });
        if !args.prompt.is_empty() {
            full.set_initial_prompt(&args.prompt);
        }
        // Ditado são falas curtas e independentes, como no Python.
        full.set_no_context(true);
        full.set_no_timestamps(true);
        full.set_print_progress(false);
        full.set_print_realtime(false);
        full.set_print_special(false);

        let t = Instant::now();
        state.full(full, &audio)?;
        let stt_s = t.elapsed().as_secs_f64();
        first_text_s.get_or_insert_with(|| started.elapsed().as_secs_f64());

        let text = state
            .as_iter()
            .map(|seg| seg.to_str_lossy().map(|s| s.trim().to_owned()))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        let lang = get_lang_str(state.full_lang_id_from_state()).unwrap_or("?");

        stt_total += stt_s;
        audio_total += audio_s;
        println!(
            "{}",
            json!({
                "file": wav.file_name().and_then(|n| n.to_str()),
                "audio_s": audio_s,
                "stt_s": stt_s,
                "language": lang,
                "text": text,
            })
        );
    }

    println!(
        "{}",
        json!({
            "summary": true,
            "engine": "whisper.cpp",
            "model": args.model.file_name().and_then(|n| n.to_str()),
            "load_s": load_s,
            "first_text_s": first_text_s,
            "files": args.wavs.len(),
            "audio_s": audio_total,
            "stt_s": stt_total,
        })
    );
    Ok(())
}
