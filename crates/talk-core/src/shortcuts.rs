//! Os atalhos do talk dentro do arquivo de atalhos do COSMIC.
//!
//! O COSMIC é dono das teclas globais, e o arquivo
//! `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom` guarda
//! atalhos de outras ferramentas também. Por isso nunca é regenerado: só o
//! `Spawn` de um atalho do talk é editado, no lugar, e o resto volta igual.
//! O compositor relê o arquivo sozinho (inotify), valendo no próximo aperto.

use std::path::PathBuf;

use regex::Regex;

use crate::keys::Combo;

pub fn cosmic_file() -> PathBuf {
    crate::paths::config_dir()
        .parent()
        .map(|p| p.join("cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom"))
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub modifiers: Vec<String>,
    pub key: String,
    pub description: Option<String>,
    pub command: String,
}

impl Binding {
    /// É do talk: chama o `talk-rs`.
    pub fn is_talk(&self) -> bool {
        let prog = self.command.split_whitespace().next().unwrap_or("");
        prog.ends_with("/talk-rs") || prog == "talk-rs"
    }

    /// "pt", "en", "auto" ou vazio (sem `-l`: vale o config).
    pub fn language(&self) -> Option<String> {
        let parts: Vec<&str> = self.command.split_whitespace().collect();
        parts.windows(2).find(|w| w[0] == "-l" || w[0] == "--language").map(|w| w[1].to_owned())
    }

    /// As teclas, no formato comum ao Windows. `None` para uma tecla que a
    /// janela não sabe editar (Print, setas...).
    pub fn combo(&self) -> Option<Combo> {
        let mut text: Vec<String> = self.modifiers.iter().map(|m| m.to_lowercase()).collect();
        text.push(self.key.clone());
        Combo::parse(&text.join("+"))
    }

    /// As teclas como se leem: Super, Alt, D.
    pub fn keys(&self) -> Vec<String> {
        let mut keys = self.modifiers.clone();
        keys.push(if self.key.chars().count() == 1 { self.key.to_uppercase() } else { self.key.clone() });
        keys
    }
}

fn pattern() -> Regex {
    Regex::new(
        r#"\(\s*modifiers:\s*\[(?P<mods>[^\]]*)\],\s*key:\s*"(?P<key>[^"]*)",\s*(?:description:\s*(?:Some\("(?P<desc>[^"]*)"\)|None),\s*)?\)\s*:\s*Spawn\("(?P<cmd>[^"]*)"\)"#,
    )
    .unwrap_or_else(|e| panic!("regex dos atalhos: {e}"))
}

pub fn parse(text: &str) -> Vec<Binding> {
    pattern()
        .captures_iter(text)
        .map(|c| Binding {
            modifiers: c["mods"]
                .split(',')
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(str::to_owned)
                .collect(),
            key: c["key"].to_owned(),
            description: c.name("desc").map(|d| d.as_str().to_owned()),
            command: c["cmd"].to_owned(),
        })
        .collect()
}

/// Troca o idioma do atalho de ditado que tem essa descrição. `None` se não
/// houver um; o texto devolvido só difere naquele `Spawn`.
pub fn set_language(text: &str, description: &str, language: &str) -> Option<String> {
    let re = pattern();
    let c = re.captures_iter(text).find(|c| c.name("desc").is_some_and(|d| d.as_str() == description))?;
    let cmd = c.name("cmd")?;
    let parts: Vec<&str> = cmd.as_str().split_whitespace().collect();
    let mut out: Vec<String> = vec![];
    let mut i = 0;
    while i < parts.len() {
        if parts[i] == "-l" || parts[i] == "--language" {
            i += 2;
            continue;
        }
        out.push(parts[i].to_owned());
        i += 1;
    }
    out.push("-l".into());
    out.push(language.into());
    let mut s = text.to_owned();
    s.replace_range(cmd.range(), &out.join(" "));
    Some(s)
}

/// Os atalhos de ditado do arquivo, pela descrição (o que os identifica).
fn talk_binding<'t>(text: &'t str, description: &str) -> Option<regex::Captures<'t>> {
    pattern().captures_iter(text).find(|c| c.name("desc").is_some_and(|d| d.as_str() == description))
}

/// Outro atalho do arquivo (do talk ou não) já usa essas teclas.
pub fn taken(text: &str, combo: &Combo, except: Option<&str>) -> bool {
    parse(text)
        .iter()
        .filter(|b| except.is_none() || b.description.as_deref() != except)
        .any(|b| b.combo().as_ref() == Some(combo))
}

fn mods_ron(combo: &Combo) -> String {
    let mods: String = combo.cosmic_mods().iter().map(|m| format!("\n            {m},")).collect();
    format!("{mods}\n        ")
}

/// A tecla no formato do COSMIC (o nome do keysym): "d", "5", "F9".
fn key_ron(combo: &Combo) -> String {
    if combo.key.len() > 1 { combo.key.to_uppercase() } else { combo.key.clone() }
}

/// Troca as teclas do atalho com essa descrição; o comando fica igual.
pub fn set_keys(text: &str, description: &str, combo: &Combo) -> Option<String> {
    let c = talk_binding(text, description)?;
    let (mods, key) = (c.name("mods")?.range(), c.name("key")?.range());
    let mut s = text.to_owned();
    // De trás para a frente: a primeira troca não desloca a segunda.
    s.replace_range(key, &key_ron(combo));
    s.replace_range(mods, &mods_ron(combo));
    Some(s)
}

/// Um atalho novo no fim do arquivo (que pode nem existir ainda).
pub fn add(text: &str, description: &str, combo: &Combo, command: &str) -> String {
    let entry = format!(
        "    (\n        modifiers: [{}],\n        key: \"{}\",\n        description: Some(\"{description}\"),\n    ): Spawn(\"{command}\"),\n",
        mods_ron(combo),
        key_ron(combo),
    );
    let Some(end) = text.rfind('}') else { return format!("{{\n{entry}}}") };
    let head = text[..end].trim_end();
    let comma = if head.ends_with(',') || head.ends_with('{') { "" } else { "," };
    format!("{head}{comma}\n{entry}{}", &text[end..])
}

/// Tira o atalho com essa descrição, com a vírgula e a linha dele.
pub fn remove(text: &str, description: &str) -> Option<String> {
    let c = talk_binding(text, description)?;
    let whole = c.get(0)?.range();
    let start = text[..whole.start].rfind('\n').map_or(whole.start, |i| i + 1);
    let rest = &text[whole.end..];
    let end = whole.end + rest.find(|ch: char| ch != ',' && ch != ' ').unwrap_or(rest.len());
    let end = if text[end..].starts_with('\n') { end + 1 } else { end };
    let mut s = text.to_owned();
    s.replace_range(start..end, "");
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{
    (
        modifiers: [
            Super,
        ],
        key: "d",
        description: Some("voice-pt"),
    ): Spawn("/home/dev/.local/bin/talk-rs toggle -l auto"),
    (
        modifiers: [
            Super,
            Alt,
        ],
        key: "d",
        description: Some("voice-en"),
    ): Spawn("/home/dev/.local/bin/talk-rs toggle -l en"),
    (
        modifiers: [],
        key: "Print",
        description: Some("imgcpy-shot"),
    ): Spawn("/home/dev/imgcpy/imgcpy shot"),
}"#;

    #[test]
    fn finds_the_talk_bindings_among_the_others() {
        let all = parse(FILE);
        assert_eq!(all.len(), 3);
        let talk: Vec<_> = all.iter().filter(|b| b.is_talk()).collect();
        assert_eq!(talk.len(), 2);
        assert_eq!(talk[1].keys(), ["Super", "Alt", "D"]);
        assert_eq!(talk[1].language().as_deref(), Some("en"));
        assert_eq!(all[2].keys(), ["Print"]);
    }

    #[test]
    fn changes_only_that_spawn() {
        let out = set_language(FILE, "voice-pt", "pt").unwrap();
        assert!(out.contains(r#"Spawn("/home/dev/.local/bin/talk-rs toggle -l pt")"#));
        assert_eq!(out.len(), FILE.len() - 2);
        assert!(set_language(FILE, "nao-existe", "pt").is_none());
    }

    #[test]
    fn changes_only_the_keys() {
        let combo = Combo::parse("ctrl+alt+f9").unwrap();
        let out = set_keys(FILE, "voice-en", &combo).unwrap();
        let all = parse(&out);
        assert_eq!(all.len(), 3);
        assert_eq!(all[1].keys(), ["Ctrl", "Alt", "F9"]);
        assert_eq!(all[1].command, "/home/dev/.local/bin/talk-rs toggle -l en");
        assert_eq!(all[0], parse(FILE)[0], "o outro atalho do talk fica igual");
        assert_eq!(all[2], parse(FILE)[2], "e o de outra ferramenta também");
    }

    #[test]
    fn adds_and_removes_keeping_the_rest() {
        let combo = Combo::parse("super+shift+d").unwrap();
        let out = add(FILE, "talk-2", &combo, "/x/talk-rs toggle -l pt");
        let all = parse(&out);
        assert_eq!(all.len(), 4);
        assert_eq!(all[3].keys(), ["Super", "Shift", "D"]);
        assert!(all[3].is_talk());
        assert_eq!(remove(&out, "talk-2").unwrap(), FILE);
        let out = remove(FILE, "voice-pt").unwrap();
        assert_eq!(parse(&out).len(), 2);
        assert_eq!(parse(&add("", "talk-1", &combo, "talk-rs toggle")).len(), 1, "arquivo que não existia");
    }

    #[test]
    fn knows_when_keys_are_taken() {
        let superd = Combo::parse("super+d").unwrap();
        assert!(taken(FILE, &superd, None));
        assert!(!taken(FILE, &superd, Some("voice-pt")), "o próprio atalho não conta");
        assert!(!taken(FILE, &Combo::parse("ctrl+alt+d").unwrap(), None));
    }
}
