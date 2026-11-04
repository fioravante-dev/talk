//! As três telas e as ações delas. Cada ação grava no arquivo de sempre
//! (config.toml, vocab.txt, os atalhos do COSMIC) e devolve a tela inteira:
//! o htmx troca o `#app`, então não existe estado na janela para dessincronizar.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use askama::Template;
use talk_core::config::{self, Config};
use talk_core::history;
use talk_core::paths;
use talk_core::shortcuts;
use talk_core::vocab::{self, Entry, Vocab};

use crate::system;

pub struct Shell {
    pub tab: &'static str,
    pub phase: String,
    pub model_loaded: bool,
    pub service_mb: String,
    pub vram_mb: String,
    pub last_stt: String,
}

/// Vírgula decimal, como se lê em português.
pub fn secs(s: f64) -> String {
    let t = if s < 10.0 { format!("{s:.2}") } else { format!("{s:.1}") };
    format!("{} s", t.replace('.', ","))
}

fn shell(tab: &'static str) -> Shell {
    let st = system::status();
    let service_mb = st.pid.and_then(system::rss_mb).map_or("—".into(), |mb| format!("{mb:.0} MB"));
    let vram_mb = system::worker_vram_mb().map_or("—".into(), |mb| format!("{mb} MB"));
    let last_stt = history::read(&paths::history_file(), 1)
        .first()
        .map_or("—".into(), |e| secs(e.stt_seconds));
    Shell { tab, phase: st.phase, model_loaded: st.model_loaded, service_mb, vram_mb, last_stt }
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("talk-tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).with_context(|| path.display().to_string())
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

// ---------------------------------------------------------------- ajustes

pub struct ShortcutRow {
    pub label: String,
    pub combo: String,
    pub keys: Vec<String>,
    pub description: Option<String>,
    pub language: String,
}

pub struct ModelChoice {
    pub file: &'static str,
    pub name: &'static str,
    pub note: &'static str,
    pub chosen: bool,
    pub present: bool,
}

pub struct MicChoice {
    pub name: String,
    pub label: String,
    pub chosen: bool,
}

#[derive(Template)]
#[template(path = "ajustes.html")]
pub struct Ajustes {
    pub shell: Shell,
    pub shortcuts: Vec<ShortcutRow>,
    pub languages: Vec<(&'static str, &'static str)>,
    pub paste_key: String,
    pub models: Vec<ModelChoice>,
    pub keep_options: Vec<(&'static str, &'static str)>,
    pub keep_alive: String,
    pub keep_index: usize,
    pub keep_note: String,
    pub mics: Vec<MicChoice>,
}

const LANGUAGES: [(&str, &str); 3] = [("auto", "detectar"), ("pt", "português"), ("en", "inglês")];
const KEEP: [(&str, &str); 5] = [("0", "0"), ("30s", "30 s"), ("5m", "5 min"), ("15m", "15 min"), ("never", "sempre")];
const MODELS: [(&str, &str, &str, &str); 2] = [
    ("ggml-large-v3-q5_0.bin", "large-v3 q5", "mais preciso · 2,2 GB", "2,2 GB"),
    ("ggml-large-v3-turbo-q8_0.bin", "turbo q8", "mais rápido · 1,3 GB", "1,3 GB"),
];

/// O valor do config na escala: o ponto mais próximo, para um "10m" escrito à
/// mão não deixar a agulha fora do mostrador.
pub fn keep_index(value: &str) -> usize {
    let Ok(d) = config::parse_duration(value) else { return 2 };
    let Some(d) = d else { return 4 };
    let s = d.as_secs_f64();
    [0.0, 30.0, 300.0, 900.0]
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - s).abs().total_cmp(&(b.1 - s).abs()))
        .map_or(2, |(i, _)| i)
}

pub fn keep_note(index: usize, size: &str) -> String {
    match index {
        0 => "A GPU volta a 0 MB logo depois de cada ditada; a próxima carrega o modelo de novo enquanto você fala.".into(),
        1 => "A GPU volta a 0 MB trinta segundos depois da última ditada.".into(),
        2 => "A GPU volta a 0 MB cinco minutos depois da última ditada.".into(),
        3 => "A GPU volta a 0 MB quinze minutos depois da última ditada.".into(),
        _ => format!("O modelo fica sempre na GPU, ocupando {size}: nenhuma ditada espera carregar."),
    }
}

fn shortcut_rows() -> Vec<ShortcutRow> {
    shortcuts::parse(&read(&shortcuts::cosmic_file()))
        .into_iter()
        .filter(shortcuts::Binding::is_talk)
        .map(|b| {
            let language = b.language().unwrap_or_else(|| "auto".into());
            let label = match language.as_str() {
                "en" => "Ditar em inglês",
                "pt" => "Ditar em português",
                _ => "Ditar, detectando o idioma",
            }
            .to_owned();
            ShortcutRow {
                label,
                keys: b.keys(),
                combo: b.keys().join("+"),
                language,
                description: b.description.clone(),
            }
        })
        .collect()
}

pub fn ajustes() -> Result<String> {
    let cfg = Config::load(&paths::config_file())?;
    let models_dir = paths::models_dir();
    let chosen_size = MODELS.iter().find(|m| m.0 == cfg.ggml_model).map_or("2,2 GB", |m| m.3);
    let models = MODELS
        .iter()
        .map(|&(file, name, note, _)| ModelChoice {
            file,
            name,
            note: if models_dir.join(file).exists() { note } else { "não baixado" },
            chosen: cfg.ggml_model == file,
            present: models_dir.join(file).exists(),
        })
        .collect();
    let (found, default) = system::mics();
    let default_label = default
        .as_ref()
        .and_then(|d| found.iter().find(|m| &m.name == d))
        .map_or("Padrão do sistema".to_owned(), |m| format!("Padrão: {}", m.description));
    let mut mics = vec![MicChoice { name: String::new(), label: default_label, chosen: cfg.input_device.is_empty() }];
    mics.extend(found.into_iter().map(|m| MicChoice {
        chosen: cfg.input_device == m.name,
        name: m.name,
        label: m.description,
    }));
    let idx = keep_index(&cfg.whisper_keep_alive);
    Ok(Ajustes {
        shell: shell("ajustes"),
        shortcuts: shortcut_rows(),
        languages: LANGUAGES.to_vec(),
        paste_key: cfg.paste_key.to_lowercase(),
        models,
        keep_options: KEEP.to_vec(),
        keep_alive: KEEP[idx].0.to_owned(),
        keep_index: idx,
        keep_note: keep_note(idx, chosen_size),
        mics,
    }
    .render()?)
}

fn set_config(key: &str, value: &str) -> Result<()> {
    let path = paths::config_file();
    let text = config::set_key(&read(&path), key, value)?;
    write_atomic(&path, &text)
}

pub fn ajustar(what: &str, form: &HashMap<String, String>) -> Result<String> {
    let value = form.get("valor").map(String::as_str).unwrap_or("");
    match what {
        "colar" if ["ctrl+v", "ctrl+shift+v"].contains(&value) => set_config("paste_key", value)?,
        "modelo" if MODELS.iter().any(|m| m.0 == value) => set_config("ggml_model", value)?,
        "carregado" if KEEP.iter().any(|k| k.0 == value) => set_config("whisper_keep_alive", value)?,
        "microfone" => set_config("input_device", value)?,
        "idioma" if LANGUAGES.iter().any(|l| l.0 == value) => {
            let path = shortcuts::cosmic_file();
            let text = read(&path);
            let desc = form.get("atalho").map(String::as_str).unwrap_or("");
            if let Some(new) = shortcuts::set_language(&text, desc, value) {
                // Um backup só, sobrescrito: o arquivo é do COSMIC, não nosso.
                std::fs::write(path.with_extension("bak-talk"), &text)?;
                write_atomic(&path, &new)?;
            }
        }
        _ => anyhow::bail!("ajuste inválido: {what}={value}"),
    }
    ajustes()
}

// -------------------------------------------------------------- histórico

pub struct Row {
    pub at: String,
    pub time: String,
    pub text: String,
    pub audio: String,
    pub stt: String,
    pub language: String,
    pub bar: u32,
    pub copied: bool,
}

pub struct Day {
    pub label: String,
    pub entries: Vec<Row>,
}

#[derive(Template)]
#[template(path = "historico.html")]
pub struct Historico {
    pub shell: Shell,
    pub query: String,
    pub total: usize,
    pub days: Vec<Day>,
    pub scale_label: String,
}

const MESES: [&str; 12] = ["jan.", "fev.", "mar.", "abr.", "maio", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."];

pub fn day_label(date: &str, today: &str, yesterday: &str) -> String {
    if date == today {
        return "hoje".into();
    }
    if date == yesterday {
        return "ontem".into();
    }
    let parts: Vec<&str> = date.split('-').collect();
    match (parts.get(1).and_then(|m| m.parse::<usize>().ok()), parts.get(2).and_then(|d| d.parse::<u32>().ok())) {
        (Some(m @ 1..=12), Some(d)) => format!("{d} de {}", MESES[m - 1]),
        _ => date.to_owned(),
    }
}

/// A régua única do histórico: a fala que 90% das ditadas não passam,
/// arredondada para um valor redondo, e não a mais longa: uma ditada de 5 min
/// achataria todas as outras. As que passam ficam com a barra cheia.
/// 5 s e 40 s ficam comparáveis na mesma escala.
pub fn scale_for_all(durations: &[f64]) -> f64 {
    let mut d: Vec<f64> = durations.to_vec();
    d.sort_by(f64::total_cmp);
    let p90 = d.get((d.len() * 9 / 10).min(d.len().saturating_sub(1))).copied().unwrap_or(0.0);
    scale_for(p90)
}

pub fn scale_for(longest: f64) -> f64 {
    [10.0, 15.0, 30.0, 60.0, 120.0, 300.0].into_iter().find(|s| longest <= *s).unwrap_or(600.0)
}

pub fn historico(query: &str, copied: Option<&str>) -> Result<String> {
    let q = query.trim().to_lowercase();
    let all = history::read(&paths::history_file(), 2000);
    let matched: Vec<_> = all.into_iter().filter(|e| q.is_empty() || e.text.to_lowercase().contains(&q)).collect();
    let total = matched.len();
    let shown: Vec<_> = matched.into_iter().take(300).collect();
    let scale = scale_for_all(&shown.iter().map(|e| e.audio_seconds).collect::<Vec<_>>());

    let now = chrono::Local::now().date_naive();
    let today = now.format("%Y-%m-%d").to_string();
    let yesterday = now.pred_opt().map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();

    let mut days: Vec<Day> = vec![];
    for e in shown {
        let (date, time) = e.at.split_once('T').unwrap_or((e.at.as_str(), ""));
        let label = day_label(date, &today, &yesterday);
        let row = Row {
            copied: copied == Some(e.at.as_str()),
            time: time.get(..5).unwrap_or(time).to_owned(),
            bar: ((e.audio_seconds / scale) * 100.0).clamp(0.0, 100.0).round() as u32,
            audio: format!("{} de fala", secs(e.audio_seconds)),
            stt: format!("{} para transcrever", secs(e.stt_seconds)),
            language: e.language.clone(),
            text: e.text.clone(),
            at: e.at.clone(),
        };
        match days.last_mut() {
            Some(d) if d.label == label => d.entries.push(row),
            _ => days.push(Day { label, entries: vec![row] }),
        }
    }
    Ok(Historico {
        shell: shell("historico"),
        query: query.trim().to_owned(),
        total,
        days,
        scale_label: if scale >= 60.0 { format!("{} min", scale / 60.0) } else { format!("{scale} s") },
    }
    .render()?)
}

pub fn copiar(form: &HashMap<String, String>) -> Result<String> {
    let at = form.get("at").cloned().unwrap_or_default();
    let query = form.get("q").cloned().unwrap_or_default();
    if let Some(e) = history::read(&paths::history_file(), 5000).into_iter().find(|e| e.at == at) {
        system::copy(&e.text)?;
    }
    historico(&query, Some(&at))
}

// ------------------------------------------------------------ vocabulário

pub struct Suggestion {
    pub word: String,
    pub count: usize,
}

#[derive(Template)]
#[template(path = "vocabulario.html")]
pub struct Vocabulario {
    pub shell: Shell,
    pub entries: Vec<Entry>,
    pub generic: Vec<String>,
    pub tokens: usize,
    pub limit: usize,
    pub budget_pct: u32,
    pub over: bool,
    pub suggestions: Vec<Suggestion>,
    pub draft: String,
    pub error: String,
}

/// O Whisper põe maiúscula depois de vírgula às vezes; estas não são nomes.
const COMUNS: &[&str] = &[
    "Mas", "Então", "Aí", "Porque", "Mais", "Agora", "Depois", "Também", "Isso", "Esse", "Essa",
    "Ele", "Ela", "Eles", "Elas", "Você", "Vocês", "Não", "Sim", "Tá", "Né", "Que", "Como", "Quando",
    "Onde", "Para", "Pra", "Com", "Sem", "Por", "Uma", "Um", "Tudo", "Nada", "Bom", "Beleza", "Opa",
    "The", "And", "But",
];

/// Palavras com maiúscula no MEIO da frase, repetidas no histórico e que o
/// vocabulário ainda não tem: nomes próprios e jargão, os que o Whisper erra.
pub fn suggest(texts: &[String], known: &[String], max: usize) -> Vec<Suggestion> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for t in texts {
        let mut sentence_start = true;
        for raw in t.split_whitespace() {
            let word: String = raw.trim_matches(|c: char| !c.is_alphanumeric()).to_owned();
            let starts_upper = word.chars().next().is_some_and(char::is_uppercase);
            if !sentence_start && starts_upper && word.chars().count() >= 3 {
                *counts.entry(word).or_default() += 1;
            }
            sentence_start = raw.ends_with(['.', '!', '?', ':']);
        }
    }
    let mut out: Vec<Suggestion> = counts
        .into_iter()
        .filter(|(w, n)| *n >= 3 && !known.iter().any(|k| k.eq_ignore_ascii_case(w)))
        .filter(|(w, _)| !COMUNS.iter().any(|c| c.eq_ignore_ascii_case(w)))
        .map(|(word, count)| Suggestion { word, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));
    out.truncate(max);
    out
}

pub fn vocabulario(draft: &str, error: &str) -> Result<String> {
    let cfg = Config::load(&paths::config_file())?;
    let file = read(&paths::vocab_file());
    let v = Vocab::new(&file, &cfg.vocab);
    let tokens = vocab::estimate_tokens(&v.prompt());
    let limit = vocab::PROMPT_TOKEN_LIMIT;
    let mut known: Vec<String> = v.entries.iter().map(|e| e.term.clone()).collect();
    known.extend(v.generic.iter().cloned());
    let texts: Vec<String> = history::read(&paths::history_file(), 400).into_iter().map(|e| e.text).collect();
    Ok(Vocabulario {
        shell: shell("vocabulario"),
        suggestions: suggest(&texts, &known, 8),
        entries: v.entries.clone(),
        generic: v.generic.clone(),
        tokens,
        limit,
        budget_pct: ((tokens as f64 / limit as f64) * 100.0).min(100.0).round() as u32,
        over: tokens > limit,
        draft: draft.to_owned(),
        error: error.to_owned(),
    }
    .render()?)
}

pub fn vocab_action(what: &str, form: &HashMap<String, String>) -> Result<String> {
    let path = paths::vocab_file();
    let file = read(&path);
    let term = form.get("termo").map(|t| t.trim().to_owned()).unwrap_or_default();
    if term.is_empty() || term.contains(['=', ',', '\n']) {
        return vocabulario(&term, "O termo não pode ficar vazio nem ter “=” ou vírgula.");
    }
    let new = match what {
        "adicionar" => {
            let errors: Vec<String> = form
                .get("erros")
                .map(|e| e.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            vocab::upsert(&file, &Entry { term, errors })
        }
        "remover" => vocab::remove(&file, &term),
        _ => anyhow::bail!("ação desconhecida: {what}"),
    };
    write_atomic(&path, &new)?;
    vocabulario("", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_read_in_portuguese() {
        assert_eq!(secs(0.6512), "0,65 s");
        assert_eq!(secs(12.84), "12,8 s");
    }

    #[test]
    fn the_needle_snaps_to_the_nearest_stop() {
        assert_eq!(keep_index("5m"), 2);
        assert_eq!(keep_index("12m"), 3);
        assert_eq!(keep_index("never"), 4);
        assert_eq!(keep_index("0"), 0);
        assert_eq!(keep_index("lixo"), 2);
    }

    #[test]
    fn days_read_as_people_say_them() {
        assert_eq!(day_label("2026-11-03", "2026-11-03", "2026-11-02"), "hoje");
        assert_eq!(day_label("2026-11-02", "2026-11-03", "2026-11-02"), "ontem");
        assert_eq!(day_label("2026-10-30", "2026-11-03", "2026-11-02"), "30 de out.");
    }

    #[test]
    fn one_scale_for_the_whole_list_that_one_outlier_cannot_flatten() {
        assert_eq!(scale_for(8.0), 10.0);
        assert_eq!(scale_for(40.2), 60.0);
        let mut d = vec![8.0; 19];
        d.push(290.0);
        assert_eq!(scale_for_all(&d), 10.0);
    }

    #[test]
    fn suggests_mid_sentence_names_it_does_not_know() {
        let texts: Vec<String> = [
            "abre o Hyprland agora. Depois o T3 Code",
            "no Hyprland tem um bug",
            "o Hyprland e o Claude",
            "fala com o Claude. Claude é bom",
            "pergunta pro Claude",
        ]
        .map(String::from)
        .to_vec();
        let got = suggest(&texts, &["claude".into()], 5);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].word.as_str(), got[0].count), ("Hyprland", 3));
    }
}
