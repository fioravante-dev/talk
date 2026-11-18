//! Procurar e aplicar a atualização (o que decidir fica em `talk_core::update`).
//!
//! Quem troca os arquivos nunca é o próprio serviço: ele solta um processo à
//! parte (um `systemd-run` no Linux, um PowerShell escondido no Windows), que
//! para o talk, troca e sobe de novo. Assim o serviço segue ditando enquanto
//! a atualização compila ou baixa, e só cai no fim, por um ou dois segundos.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use talk_core::paths;
use talk_core::update::{self, Method, Release, State};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn command(program: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new(format!("{program}.exe"));
        c.creation_flags(crate::worker::NO_WINDOW);
        c
    }
    #[cfg(not(windows))]
    Command::new(program)
}

fn curl_text(url: &str) -> Result<String> {
    let out = command("curl")
        .args(["-fsSL", "--max-time", "20", "-A", "talk-update", "-H", "Accept: application/vnd.github+json", url])
        .output()
        .context("curl não encontrado")?;
    if !out.status.success() {
        bail!("não deu para falar com o GitHub: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn fetch() -> Result<Release> {
    update::parse_release(&curl_text(&update::latest_url())?)
}

/// Procura agora e guarda o resultado (ou o erro) no `update.json`.
pub fn check() -> Result<State> {
    let mut state = State::load();
    let found = fetch();
    match &found {
        Ok(r) => state.record(r, now()),
        Err(e) => {
            state.checked = now();
            state.error = Some(format!("{e:#}"));
        }
    }
    state.save()?;
    found.map(|_| state)
}

/// Atualiza para a última versão: começa o trabalho e volta logo, com o que
/// dizer à pessoa. O talk reinicia sozinho no fim.
pub fn apply() -> Result<String> {
    let release = fetch()?;
    if !update::is_newer(&release.version, update::VERSION) {
        bail!("já está na última versão ({})", update::VERSION);
    }
    let mut state = State::load();
    state.record(&release, now());
    state.tried = Some(release.version.clone());
    state.save()?;
    let started = match update::method() {
        Method::Source(src) => from_source(&src, &release.version),
        #[cfg(windows)]
        Method::Package => from_release(&release),
        #[cfg(not(windows))]
        Method::Package => bail!("instalação desconhecida"),
        Method::Manual => bail!(
            "o talk não sabe de onde foi instalado: rode o scripts/install.sh de novo (ele também atualiza)"
        ),
    };
    if let Err(e) = &started {
        state.error = Some(format!("{e:#}"));
        let _ = state.save();
    }
    started
}

fn log_file() -> std::path::PathBuf {
    paths::cache_dir().join("update.log")
}

/// `git pull` e o instalador de novo, num processo solto do serviço: o
/// instalador para o talk no fim, e não pode morrer junto.
#[cfg(not(windows))]
fn from_source(src: &Path, version: &str) -> Result<String> {
    const SCRIPT: &str = r#"
cd "$1" || exit 1
echo "== $(date '+%F %T') atualizando para $2"
if git pull --ff-only && TALK_UPDATE=1 ./scripts/install.sh; then exit 0; fi
notify-send -a talk "⚠ A atualização do talk falhou" "Nada mudou. Veja $3"
exit 1
"#;
    std::fs::create_dir_all(paths::cache_dir())?;
    let log = log_file();
    let home = std::env::var("HOME").unwrap_or_default();
    // O serviço sobe no login com o PATH do ambiente gráfico, que pode não ter o cargo.
    let path = format!("{home}/.cargo/bin:/usr/local/cuda/bin:{}", std::env::var("PATH").unwrap_or_default());
    let args = ["bash", "-c", SCRIPT, "talk-update", &src.display().to_string(), version, &log.display().to_string()];
    let unit = format!("talk-update-{}", now());
    let via_systemd = Command::new("systemd-run")
        .args(["--user", "--quiet", "--collect", &format!("--unit={unit}")])
        .arg(format!("--setenv=PATH={path}"))
        .arg(format!("--property=StandardOutput=append:{}", log.display()))
        .arg(format!("--property=StandardError=append:{}", log.display()))
        .args(args)
        .status();
    if !via_systemd.is_ok_and(|s| s.success()) {
        let out = std::fs::OpenOptions::new().create(true).append(true).open(&log)?;
        Command::new("setsid")
            .args(args)
            .env("PATH", &path)
            .stdin(std::process::Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out)
            .spawn()
            .context("não deu para começar a atualização")?;
    }
    Ok(format!("Compilando a {version} para a sua placa: leva alguns minutos, e o talk reinicia sozinho."))
}

/// Uma string para dentro de aspas simples do PowerShell.
#[cfg(windows)]
fn ps(s: impl AsRef<std::ffi::OsStr>) -> String {
    format!("'{}'", s.as_ref().to_string_lossy().replace('\'', "''"))
}

/// Roda um script do PowerShell escondido e solto do serviço, com a saída no `update.log`.
#[cfg(windows)]
fn spawn_powershell(script: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let dir = paths::cache_dir().join("update");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("aplicar.ps1");
    // Com BOM: o PowerShell 5 lê um .ps1 sem BOM como ANSI e estraga os acentos.
    std::fs::write(&file, format!("\u{feff}{script}"))?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(log_file())?;
    Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"])
        .arg(&file)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .creation_flags(crate::worker::NO_WINDOW | crate::worker::DETACHED)
        .spawn()
        .context("não deu para começar a atualização")?;
    Ok(())
}

/// O aviso de falha, pelo próprio PowerShell (o serviço já pode ter saído).
#[cfg(windows)]
const PS_FAIL: &str = r#"
# A saída vai para o update.log: em UTF-8, como o resto do talk.
[Console]::OutputEncoding = [Text.Encoding]::UTF8
function Falhou($msg) {
    Write-Host "falhou: $msg"
    [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
    $xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02)
    $t = $xml.GetElementsByTagName('text')
    $t.Item(0).AppendChild($xml.CreateTextNode('A atualização do talk falhou')) | Out-Null
    $t.Item(1).AppendChild($xml.CreateTextNode($msg)) | Out-Null
    $id = '{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe'
    [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($id).Show([Windows.UI.Notifications.ToastNotification]::new($xml))
    exit 1
}
"#;

#[cfg(windows)]
fn from_source(src: &Path, version: &str) -> Result<String> {
    let script = format!(
        r#"{PS_FAIL}
Write-Host "== $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') atualizando para {v}"
$env:TALK_UPDATE = '1'
Set-Location {src}
git pull --ff-only
if ($LASTEXITCODE -ne 0) {{ Falhou 'o git pull falhou; nada mudou' }}
try {{ & .\scripts\install.ps1 }} catch {{ Falhou "$_" }}
"#,
        v = version,
        src = ps(src),
    );
    spawn_powershell(&script)?;
    Ok(format!("Compilando a {version} para a sua placa: leva alguns minutos, e o talk reinicia sozinho."))
}

#[cfg(windows)]
fn sha256(file: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(file)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Pelo instalador pronto: só os executáveis, se o CUDA instalado serve;
/// senão, o instalador inteiro, em modo silencioso.
#[cfg(windows)]
fn from_release(release: &Release) -> Result<String> {
    use talk_core::update::Download;
    let exe = std::env::current_exe()?;
    let app = exe.parent().context("sem a pasta do talk")?.to_owned();
    let dl = update::windows_download(release, update::cuda_major(&app))
        .context("a release não tem o pacote do Windows")?;
    let asset = dl.asset();
    let sum = update::checksum_asset(release, asset)
        .context("a release não tem o .sha256: sem conferir, o talk não instala")?;
    let expected = update::parse_sha256(&curl_text(&sum.url)?).context(".sha256 ilegível")?;

    let dir = paths::cache_dir().join("update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(&asset.name);
    let ok = command("curl")
        .args(["-fL", "--retry", "2", "-A", "talk-update", "-o"])
        .arg(&file)
        .arg(&asset.url)
        .status()
        .context("curl não encontrado")?;
    if !ok.success() {
        bail!("o download de {} falhou", asset.name);
    }
    if sha256(&file)? != expected {
        let _ = std::fs::remove_file(&file);
        bail!("{} chegou diferente do publicado (sha256)", asset.name);
    }

    let stop = format!(
        r#"Start-Sleep -Milliseconds 800
Get-Process talk-rs, talk-ui, talk-pill -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 700
Remove-Item (Join-Path {cache} 'talk-rs.sock'), (Join-Path {cache} 'talk-ui.sock') -ErrorAction SilentlyContinue"#,
        cache = ps(paths::cache_dir()),
    );
    let (script, what) = match dl {
        Download::Partial(_) => {
            let new = dir.join("novo");
            std::fs::create_dir_all(&new)?;
            let ok = command("tar").arg("-xf").arg(&file).arg("-C").arg(&new).status().context("tar não encontrado")?;
            if !ok.success() || !new.join("talk-rs.exe").exists() {
                bail!("{} veio quebrado", asset.name);
            }
            let script = format!(
                r#"{PS_FAIL}
Write-Host "== $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') atualizando para {v} (só os programas)"
{stop}
$ok = $false
foreach ($i in 1..10) {{
    try {{ Copy-Item (Join-Path {new} '*') {app} -Force -ErrorAction Stop; $ok = $true; break }}
    catch {{ Start-Sleep -Milliseconds 500 }}
}}
Start-Process (Join-Path {app} 'talk-start.exe')
if (-not $ok) {{ Falhou 'um arquivo do talk estava em uso; tente de novo' }}
Write-Host 'pronto'
"#,
                v = release.version,
                new = ps(&new),
                app = ps(&app),
            );
            (script, format!("só os programas, {} MB", asset.size / 1_000_000))
        }
        Download::Full(_) => {
            let script = format!(
                r#"{PS_FAIL}
Write-Host "== $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') atualizando para {v} (instalador inteiro)"
{stop}
$p = Start-Process {setup} -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru
Start-Process (Join-Path {app} 'talk-start.exe')
if ($p.ExitCode -ne 0) {{ Falhou "o instalador saiu com $($p.ExitCode)" }}
Write-Host 'pronto'
"#,
                v = release.version,
                setup = ps(&file),
                app = ps(&app),
            );
            (script, format!("o instalador inteiro, {} MB", asset.size / 1_000_000))
        }
    };
    spawn_powershell(&script)?;
    Ok(format!("Baixado ({what}): o talk reinicia na {} em instantes.", release.version))
}
