//! A atualização, na parte que é só decidir: que versão é esta, qual é a
//! última publicada no GitHub, o que baixar e o que já se sabe.
//!
//! Baixar e trocar os arquivos fica no `talk-rs` (`update.rs`). Os dois
//! caminhos:
//!
//! - **Do código** (Linux sempre; Windows pelo `install.ps1`): `git pull` e o
//!   instalador de novo, que recompila só o que mudou para a placa daqui. O
//!   CUDA, o Rust, o whisper.cpp já compilado e o modelo ficam.
//! - **Pelo instalador pronto** (Windows): se a DLL do cuBLAS instalada é da
//!   mesma versão do CUDA da release, baixa só os executáveis
//!   (`talk-app-<versão>-cuda<N>.zip`); senão, o instalador inteiro.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

pub const REPO: &str = "fioravante-dev/talk";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const COMMIT: &str = env!("TALK_COMMIT");

/// "0.3.0 (7a77b72)", ou "0.3.0 (7a77b72, modificado)" se o código tinha
/// mudanças fora do commit.
pub fn version_line() -> String {
    if COMMIT.is_empty() { VERSION.to_owned() } else { format!("{VERSION} ({COMMIT})") }
}

/// De onde vem a última release. `TALK_UPDATE_URL` troca (um `file://`, nos testes).
pub fn latest_url() -> String {
    std::env::var("TALK_UPDATE_URL")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| format!("https://api.github.com/repos/{REPO}/releases/latest"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// Sem o "v" da tag.
    pub version: String,
    pub notes: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// O JSON de `releases/latest` (o GitHub já deixa de fora rascunhos e prévias).
pub fn parse_release(json: &str) -> Result<Release> {
    let r: GhRelease = serde_json::from_str(json).context("resposta do GitHub inválida")?;
    Ok(Release {
        version: r.tag_name.trim_start_matches('v').to_owned(),
        notes: r.body.unwrap_or_default(),
        page: r.html_url,
        assets: r.assets,
    })
}

fn triple(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().trim_start_matches('v').split(['-', '+', ' ']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

/// `candidate` é mais nova que `current`? Uma versão que não dá para ler nunca é.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((triple(candidate), triple(current)), (Some(a), Some(b)) if a > b)
}

/// O que baixar no Windows instalado pelo instalador pronto.
#[derive(Debug, PartialEq)]
pub enum Download<'a> {
    /// Só os executáveis: o cuBLAS instalado serve.
    Partial(&'a Asset),
    /// O instalador inteiro (outro CUDA, ou a release não tem o pacote parcial).
    Full(&'a Asset),
}

impl Download<'_> {
    pub fn asset(&self) -> &Asset {
        match self {
            Download::Partial(a) | Download::Full(a) => a,
        }
    }
}

pub fn windows_download(release: &Release, cuda_major: Option<u32>) -> Option<Download<'_>> {
    let find = |name: String| release.assets.iter().find(|a| a.name == name);
    if let Some(a) = cuda_major.and_then(|n| find(format!("talk-app-{}-cuda{n}.zip", release.version))) {
        return Some(Download::Partial(a));
    }
    find(format!("talk-setup-{}.exe", release.version)).map(Download::Full)
}

/// O `<arquivo>.sha256` que acompanha cada arquivo da release.
pub fn checksum_asset<'a>(release: &'a Release, asset: &Asset) -> Option<&'a Asset> {
    let name = format!("{}.sha256", asset.name);
    release.assets.iter().find(|a| a.name == name)
}

/// "abc123...  arquivo" (sha256sum) ou só o hash (Get-FileHash): o hash, em minúsculas.
pub fn parse_sha256(text: &str) -> Option<String> {
    let h = text.split_whitespace().next()?.to_lowercase();
    (h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
}

/// A versão do CUDA do cuBLAS ao lado dos executáveis (`cublas64_13.dll` → 13).
pub fn cuda_major(dir: &Path) -> Option<u32> {
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        name.strip_prefix("cublas64_")?.strip_suffix(".dll")?.parse().ok()
    })
}

/// O que já se sabe, em `update.json` no cache: quando olhou, o que achou, e
/// de qual versão já avisou (o aviso sai uma vez por versão).
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Segundos desde 1970.
    #[serde(default)]
    pub checked: i64,
    #[serde(default)]
    pub latest: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub notified: Option<String>,
    /// Para qual versão a atualização já começou: o modo `auto` tenta uma vez
    /// só por versão, em vez de recompilar a cada hora se falhar.
    #[serde(default)]
    pub tried: Option<String>,
    /// O último erro (de procurar ou de atualizar), para a janela mostrar.
    #[serde(default)]
    pub error: Option<String>,
}

/// Uma olhada por dia.
const EVERY: i64 = 24 * 3600;

pub fn state_file() -> PathBuf {
    paths::cache_dir().join("update.json")
}

impl State {
    pub fn load() -> Self {
        std::fs::read_to_string(state_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = state_file();
        std::fs::create_dir_all(paths::cache_dir())?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn due(&self, now: i64) -> bool {
        now - self.checked >= EVERY || now < self.checked
    }

    /// A versão nova que espera, se houver.
    pub fn available(&self) -> Option<&str> {
        self.latest.as_deref().filter(|v| is_newer(v, VERSION))
    }

    pub fn record(&mut self, release: &Release, now: i64) {
        self.checked = now;
        self.latest = Some(release.version.clone());
        self.notes = release.notes.clone();
        self.error = None;
    }
}

/// Como o talk foi instalado. O `install.sh` e o `install.ps1` escrevem
/// `install.toml` no cache com a pasta do código.
#[derive(Debug, Clone, PartialEq)]
pub enum Method {
    /// `git pull` e o instalador de novo, nesta pasta.
    Source(PathBuf),
    /// Windows, pelo instalador pronto: baixa da release.
    Package,
    /// Não dá para atualizar sozinho (sem git, no Linux): o instalador à mão.
    Manual,
}

#[derive(Debug, Default, Deserialize)]
struct InstallRecord {
    #[serde(default)]
    src: Option<PathBuf>,
}

pub fn install_file() -> PathBuf {
    paths::cache_dir().join("install.toml")
}

pub fn method_from(record: Option<&str>, windows: bool) -> Method {
    let src = record.and_then(|t| toml::from_str::<InstallRecord>(t).ok()).and_then(|r| r.src);
    match src {
        Some(s) if s.join(".git").exists() => Method::Source(s),
        _ if windows => Method::Package,
        _ => Method::Manual,
    }
}

pub fn method() -> Method {
    method_from(std::fs::read_to_string(install_file()).ok().as_deref(), cfg!(windows))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(assets: &[&str]) -> Release {
        Release {
            version: "0.3.0".into(),
            notes: String::new(),
            page: String::new(),
            assets: assets
                .iter()
                .map(|n| Asset { name: (*n).into(), url: format!("https://x/{n}"), size: 1 })
                .collect(),
        }
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("v0.3.0", "0.2.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("lixo", "0.2.0"));
    }

    #[test]
    fn reads_the_github_release() {
        let json = r#"{"tag_name":"v0.3.0","body":"novidades","html_url":"https://github.com/x",
            "assets":[{"name":"talk-setup-0.3.0.exe","browser_download_url":"https://d/s.exe","size":9}]}"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.version, "0.3.0");
        assert_eq!(r.notes, "novidades");
        assert_eq!(r.assets[0].url, "https://d/s.exe");
    }

    #[test]
    fn same_cuda_downloads_only_the_programs() {
        let r = release(&["talk-setup-0.3.0.exe", "talk-app-0.3.0-cuda13.zip", "talk-app-0.3.0-cuda13.zip.sha256"]);
        let d = windows_download(&r, Some(13)).unwrap();
        assert!(matches!(d, Download::Partial(a) if a.name == "talk-app-0.3.0-cuda13.zip"));
        assert_eq!(checksum_asset(&r, d.asset()).unwrap().name, "talk-app-0.3.0-cuda13.zip.sha256");
        // Outro CUDA (ou nenhum instalado): o instalador inteiro.
        assert!(matches!(windows_download(&r, Some(12)), Some(Download::Full(_))));
        assert!(matches!(windows_download(&r, None), Some(Download::Full(_))));
        assert!(windows_download(&release(&[]), Some(13)).is_none());
    }

    #[test]
    fn checksum_text() {
        let h = "A".repeat(64);
        assert_eq!(parse_sha256(&format!("{h}  talk.zip\n")), Some("a".repeat(64)));
        assert_eq!(parse_sha256("curto"), None);
    }

    #[test]
    fn finds_the_installed_cuda() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(cuda_major(dir.path()), None);
        std::fs::write(dir.path().join("cublasLt64_13.dll"), "").unwrap();
        std::fs::write(dir.path().join("cublas64_13.dll"), "").unwrap();
        assert_eq!(cuda_major(dir.path()), Some(13));
    }

    #[test]
    fn checks_once_a_day_and_knows_what_waits() {
        let mut s = State::default();
        assert!(s.due(1_000_000));
        s.record(&release(&[]), 1_000_000);
        assert!(!s.due(1_000_000 + 3600));
        assert!(s.due(1_000_000 + EVERY));
        s.latest = Some("999.0.0".into());
        assert_eq!(s.available(), Some("999.0.0"));
        s.latest = Some(VERSION.into());
        assert_eq!(s.available(), None);
    }

    #[test]
    fn how_it_was_installed() {
        let dir = tempfile::tempdir().unwrap();
        let rec = format!("src = {:?}\n", dir.path());
        assert_eq!(method_from(Some(&rec), false), Method::Manual, "sem .git");
        assert_eq!(method_from(Some(&rec), true), Method::Package);
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert_eq!(method_from(Some(&rec), false), Method::Source(dir.path().to_owned()));
        assert_eq!(method_from(None, false), Method::Manual);
    }
}
