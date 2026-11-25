# Instala (ou atualiza) o talk no Windows a partir do código, do zero até o
# serviço rodando. Para quem não quer o instalador pronto, ou para um agente
# de IA seguir sem perguntar nada:
#
#   irm https://raw.githubusercontent.com/fioravante-dev/talk/main/scripts/install.ps1 | iex
#   .\scripts\install.ps1          # de dentro de um checkout
#
# Pode rodar de novo: só instala o que falta e recompila. O winget pede
# confirmação (UAC) para o CUDA e o Visual Studio. Precisa de uma placa NVIDIA
# RTX com o driver instalado.
#
# Variáveis: $env:TALK_DIR (onde fica o código, padrão ~\src\talk) e
# $env:CUDA_ARCH (padrão: a da placa, pelo nvidia-smi). $env:TALK_SPEECH=1
# instala também a fala (`talk-rs speak`, ~3 GB a mais). $env:TALK_UPDATE=1 é o
# modo da atualização pela bandeja (`talk-rs update`): roda escondido, então
# falha em vez de abrir o winget se faltar alguma ferramenta.

$ErrorActionPreference = "Stop"
function Say($t) { Write-Host "[talk] $t" -ForegroundColor Cyan }

$repo = "https://github.com/fioravante-dev/talk.git"

# 1. A placa.
$smi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
if (-not $smi) { throw "nvidia-smi não encontrado: instale o driver da NVIDIA primeiro" }
if (-not $env:CUDA_ARCH) {
    $env:CUDA_ARCH = ((& nvidia-smi --query-gpu=compute_cap --format=csv,noheader | Select-Object -First 1) -replace "[ .]", "")
}
Say "placa: $(& nvidia-smi --query-gpu=name --format=csv,noheader | Select-Object -First 1) (sm_$env:CUDA_ARCH)"

# 2. As ferramentas, pelo winget, só as que faltam.
function Need($id, $test, $extra) {
    if (& $test) { return }
    if ($env:TALK_UPDATE) { throw "falta $id: rode o scripts\install.ps1 num terminal" }
    Say "instalando $id"
    $a = @("install", "--id", $id, "-e", "--accept-package-agreements", "--accept-source-agreements") + $extra
    & winget @a
    if ($LASTEXITCODE -ne 0) { throw "o winget não instalou $id" }
}
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
Need "Git.Git" { Get-Command git -ErrorAction SilentlyContinue } @()
Need "Rustlang.Rustup" { Test-Path "$env:USERPROFILE\.cargo\bin\cargo.exe" } @()
Need "LLVM.LLVM" { Test-Path "C:\Program Files\LLVM\bin\libclang.dll" } @()
Need "Kitware.CMake" { (Get-Command cmake -ErrorAction SilentlyContinue) -or (Test-Path "C:\Program Files\CMake\bin\cmake.exe") } @()
Need "Nvidia.CUDA" { [Environment]::GetEnvironmentVariable("CUDA_PATH", "Machine") } @()
Need "Microsoft.VisualStudio.2022.BuildTools" {
    (Test-Path $vswhere) -and (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
} @("--override", "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive --wait")
$env:Path = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [Environment]::GetEnvironmentVariable("Path", "User")

# 3. O código: este checkout, ou um clone em TALK_DIR.
$here = if ($PSScriptRoot) { Split-Path $PSScriptRoot } else { $null }
if ($here -and (Test-Path "$here\crates\talk\Cargo.toml")) {
    $src = $here
} else {
    $src = if ($env:TALK_DIR) { $env:TALK_DIR } else { "$env:USERPROFILE\src\talk" }
    if (Test-Path "$src\.git") { Say "atualizando $src"; git -C $src pull --ff-only }
    else { Say "baixando o código em $src"; git clone --depth 1 $repo $src }
    if ($LASTEXITCODE -ne 0) { throw "o git falhou" }
}

# 4. Compila, instala (cópia, modelos, login) e sobe.
Say "compilando (a primeira vez leva uns 10 minutos: é o whisper.cpp com CUDA)"
# O cargo escreve o progresso no stderr; com "Stop" o PowerShell 5 o pinta de erro.
$ErrorActionPreference = "Continue"
& "$src\scripts\build-windows.ps1"
if ($LASTEXITCODE -ne 0) { throw "o build falhou" }
$ErrorActionPreference = "Stop"
& "$src\scripts\install-windows.ps1"
# A fala (opcional): só com $env:TALK_SPEECH=1, ou se já está instalada (aí a
# atualização a mantém em dia). $env:TALK_SPEECH=0 pula sempre.
$wantSpeech = $env:TALK_SPEECH
if (-not $wantSpeech -and (Test-Path "$env:LOCALAPPDATA\talk\speech\venv\Scripts\python.exe")) { $wantSpeech = "1" }
if ($wantSpeech -eq "1") { & "$src\scripts\install-speech.ps1" }
# De onde o `talk-rs update` puxa e recompila.
$record = "# Escrito pelo scripts\install.ps1: de onde o talk-rs update puxa e recompila.`nsrc = '$src'`ncuda_arch = '$env:CUDA_ARCH'`n"
[IO.File]::WriteAllText("$env:LOCALAPPDATA\talk\install.toml", $record)
Say "pronto: Ctrl+Alt+D dita (detectando o idioma), Ctrl+Alt+Shift+D em inglês. Troque nos Ajustes."
