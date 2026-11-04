//! Os atalhos do talk dentro do arquivo de atalhos do COSMIC.
//!
//! O COSMIC é dono das teclas globais, e o arquivo
//! `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom` guarda
//! atalhos de outras ferramentas também. Por isso nunca é regenerado: só o
//! `Spawn` de um atalho do talk é editado, no lugar, e o resto volta igual.
//! O compositor relê o arquivo sozinho (inotify), valendo no próximo aperto.

use std::path::PathBuf;

use regex::Regex;

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
}
