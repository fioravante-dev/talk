//! O pedaço do `~/.config/talk/config.toml` que o talk em Rust usa.
//!
//! Chaves desconhecidas são ignoradas: o arquivo ainda carrega as da versão em
//! Python, e elas não atrapalham.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Vazio = o modelo detecta. Um atalho com `-l` vence isto.
    pub language: String,
    /// O arquivo ggml em `~/.cache/talk/models`.
    pub ggml_model: String,
    pub beam_size: i32,
    /// `ctrl+v` ou `ctrl+shift+v` (terminais).
    pub paste_key: String,
    /// Quanto tempo o trabalhador fica vivo, ocioso: `30s`, `5m`, `0`, `never`.
    pub whisper_keep_alive: String,
    /// A dica antiga, de antes do `vocab.txt`. Soma-se a ele.
    pub vocab: String,
    /// `auto`/`paste` cola no cursor; `clipboard` só copia; `stdout` só
    /// registra no log. `TALK_TO` no ambiente vence o arquivo, como no Python.
    pub to: String,
    /// O microfone (`node.name` do PipeWire). Vazio = o padrão do sistema.
    /// Mesma chave do Python.
    pub input_device: String,
    /// Só no Windows, onde não há atalho do sistema que rode um comando e o
    /// próprio serviço registra as teclas: `"ctrl+alt+d" = "auto"`, uma tecla
    /// por idioma. No Linux quem cuida é o COSMIC (ver `shortcuts`).
    pub hotkeys: BTreeMap<String, String>,
}

/// O Super+D é do Windows (mostra a área de trabalho) e não pode ser registrado.
fn default_hotkeys() -> BTreeMap<String, String> {
    if cfg!(windows) {
        [("ctrl+alt+d", "auto"), ("ctrl+alt+shift+d", "en")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    } else {
        BTreeMap::new()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: String::new(),
            ggml_model: "ggml-large-v3-q5_0.bin".into(),
            beam_size: 5,
            paste_key: "ctrl+v".into(),
            whisper_keep_alive: "5m".into(),
            vocab: String::new(),
            to: "auto".into(),
            input_device: String::new(),
            hotkeys: default_hotkeys(),
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Self> {
        let mut cfg: Self = toml::from_str(text).context("config.toml inválido")?;
        if cfg.language == "auto" {
            cfg.language.clear();
        }
        if let Ok(to) = std::env::var("TALK_TO") {
            cfg.to = to;
        }
        Ok(cfg)
    }

    /// Sem arquivo, os padrões: o talk funciona antes do `talk --init`.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| path.display().to_string()),
        }
    }

    /// `None` = nunca descarrega.
    pub fn keep_alive(&self) -> Result<Option<Duration>> {
        parse_duration(&self.whisper_keep_alive)
    }
}

/// Troca UMA chave no texto do config, preservando comentários e o resto do
/// arquivo byte a byte: os comentários são a documentação de cada chave, e o
/// arquivo pode ter chaves que este código não conhece. Chave ausente é acrescentada no fim.
pub fn set_key(text: &str, key: &str, value: impl Into<toml_edit::Value>) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("config.toml inválido")?;
    let value = value.into();
    match doc.get_mut(key).and_then(|item| item.as_value_mut()) {
        Some(old) => {
            // O comentário na mesma linha (`key = "x"   # explica`) sobrevive.
            let decor = old.decor().clone();
            *old = value;
            *old.decor_mut() = decor;
        }
        None => {
            doc[key] = toml_edit::Item::Value(value);
        }
    }
    Ok(doc.to_string())
}

/// Troca o idioma de um atalho do `[hotkeys]` (Windows), preservando o resto
/// do arquivo. Sem a tabela no arquivo valem os padrões (`current`): eles são
/// escritos primeiro, para que mexer num atalho não apague o outro.
pub fn set_hotkey(text: &str, current: &BTreeMap<String, String>, combo: &str, language: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("config.toml inválido")?;
    if !doc.contains_table("hotkeys") {
        let mut table = toml_edit::Table::new();
        for (k, v) in current {
            table[k.as_str()] = toml_edit::value(v.as_str());
        }
        doc["hotkeys"] = toml_edit::Item::Table(table);
    }
    doc["hotkeys"][combo] = toml_edit::value(language);
    Ok(doc.to_string())
}

/// "ctrl+alt+shift+d" → Ctrl, Alt, Shift, D: como se lê no teclado.
pub fn hotkey_keys(combo: &str) -> Vec<String> {
    combo
        .split('+')
        .map(|k| match k.trim().to_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl".to_owned(),
            "win" | "windows" | "super" => "Win".to_owned(),
            other => {
                let mut c = other.chars();
                c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
            }
        })
        .collect()
}

pub fn parse_duration(text: &str) -> Result<Option<Duration>> {
    let t = text.trim();
    if t == "never" || t == "-1" {
        return Ok(None);
    }
    let (num, unit) = t.split_at(t.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(t.len()));
    let n: f64 = num.trim().parse().with_context(|| format!("duração inválida: {text:?}"))?;
    let secs = match unit {
        "" | "s" => n,
        "m" => n * 60.0,
        "h" => n * 3600.0,
        _ => bail!("duração inválida: {text:?} (use 30s, 5m, 1h, 0 ou never)"),
    };
    if secs < 0.0 {
        bail!("duração negativa: {text:?}");
    }
    Ok(Some(Duration::from_secs_f64(secs)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_shared_file_and_ignores_the_python_keys() {
        let cfg = Config::parse(
            r#"
            model = "large-v3-turbo"
            mode = "raw"
            language = ""
            paste_key = "ctrl+shift+v"
            wake = false
            "#,
        )
        .unwrap();
        assert_eq!(cfg.paste_key, "ctrl+shift+v");
        assert_eq!(cfg.ggml_model, "ggml-large-v3-q5_0.bin");
        assert_eq!(cfg.language, "");
    }

    #[test]
    fn auto_means_detect() {
        assert_eq!(Config::parse("language = \"auto\"").unwrap().language, "");
    }

    #[test]
    fn missing_file_is_the_defaults() {
        let cfg = Config::load(Path::new("/nao/existe/config.toml")).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn set_key_keeps_the_comments_and_the_other_keys() {
        let text = "# topo\nmodel = \"large-v3-turbo\"   # tiny, base\npaste_key = \"ctrl+v\"       # terminais\n";
        let out = set_key(text, "paste_key", "ctrl+shift+v").unwrap();
        assert_eq!(
            out,
            "# topo\nmodel = \"large-v3-turbo\"   # tiny, base\npaste_key = \"ctrl+shift+v\"       # terminais\n"
        );
        let out = set_key(&out, "ggml_model", "ggml-large-v3-turbo-q8_0.bin").unwrap();
        assert!(out.ends_with("ggml_model = \"ggml-large-v3-turbo-q8_0.bin\"\n"));
        assert_eq!(Config::parse(&out).unwrap().paste_key, "ctrl+shift+v");
    }

    #[test]
    fn hotkeys_map_a_combo_to_a_language() {
        let cfg = Config::parse("[hotkeys]\n\"ctrl+alt+d\" = \"auto\"\n\"ctrl+alt+e\" = \"en\"\n").unwrap();
        assert_eq!(cfg.hotkeys.get("ctrl+alt+e").map(String::as_str), Some("en"));
        assert_eq!(cfg.hotkeys.len(), 2);
    }

    #[test]
    fn set_hotkey_keeps_the_other_shortcut_and_the_comments() {
        let defaults: BTreeMap<String, String> =
            [("ctrl+alt+d", "auto"), ("ctrl+alt+shift+d", "en")].map(|(k, v)| (k.into(), v.into())).into();
        // Sem tabela: os padrões entram junto, e só o pedido muda.
        let out = set_hotkey("# topo\npaste_key = \"ctrl+v\"\n", &defaults, "ctrl+alt+d", "pt").unwrap();
        assert!(out.starts_with("# topo\n"));
        let cfg = Config::parse(&out).unwrap();
        assert_eq!(cfg.hotkeys.get("ctrl+alt+d").map(String::as_str), Some("pt"));
        assert_eq!(cfg.hotkeys.get("ctrl+alt+shift+d").map(String::as_str), Some("en"));
        // Com tabela: troca no lugar.
        let out = set_hotkey(&out, &defaults, "ctrl+alt+shift+d", "auto").unwrap();
        let cfg = Config::parse(&out).unwrap();
        assert_eq!(cfg.hotkeys.get("ctrl+alt+shift+d").map(String::as_str), Some("auto"));
        assert_eq!(cfg.hotkeys.len(), 2);
    }

    #[test]
    fn hotkeys_read_as_on_the_keyboard() {
        assert_eq!(hotkey_keys("ctrl+alt+shift+d"), ["Ctrl", "Alt", "Shift", "D"]);
        assert_eq!(hotkey_keys("win+f9"), ["Win", "F9"]);
    }

    #[test]
    fn durations_like_the_python_side() {
        assert_eq!(parse_duration("5m").unwrap(), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("30s").unwrap(), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("0").unwrap(), Some(Duration::ZERO));
        assert_eq!(parse_duration("1h").unwrap(), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("never").unwrap(), None);
        assert!(parse_duration("5 minutos").is_err());
    }
}
