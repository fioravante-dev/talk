# Gera o instalador do talk para Windows: target\dist\talk-setup-<versão>.exe.
#
#   .\scripts\package-windows.ps1
#
# Compila para as placas RTX 20xx a 50xx (o build do dia a dia, só para a placa
# daqui, é o build-windows.ps1), junta as DLLs do cuBLAS e chama o Inno Setup 6
# (winget install JRSoftware.InnoSetup).

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot)

# 75 = RTX 20xx, 86 = 30xx, 89 = 40xx, 120 = 50xx.
if (-not $env:CUDA_ARCH) { $env:CUDA_ARCH = "75;86;89;120" }
# Numa pasta própria: o CMake do whisper.cpp não recompila quando só a lista de
# placas muda, e o build do dia a dia (só a placa daqui) continua no target\release.
$env:CARGO_TARGET_DIR = "target\package"
$ErrorActionPreference = "Continue"
# O whisper.cpp sempre do zero: o CMake dele não percebe quando só as flags
# ou a lista de placas mudam, e um pacote com o build velho sairia lento.
& .\scripts\build-windows.ps1 clean --release -p whisper-rs-sys
& .\scripts\build-windows.ps1 build --release -p talk -p talk-ui -p talk-pill
if ($LASTEXITCODE -ne 0) { throw "o build falhou" }
$ErrorActionPreference = "Stop"

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$dist = "target\dist"
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $dist | Out-Null
foreach ($exe in "talk-rs.exe", "talk-start.exe", "talk-ui.exe", "talk-pill.exe") {
    Copy-Item "target\package\release\$exe" $dist
}
$cuda = [Environment]::GetEnvironmentVariable("CUDA_PATH", "Machine")
if (-not $cuda) { throw "CUDA_PATH não definido: o instalador precisa das DLLs do cuBLAS" }
Get-ChildItem "$cuda\bin" -Recurse -Filter "cublas*64_*.dll" | Copy-Item -Destination $dist

$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 não encontrado (winget install JRSoftware.InnoSetup)" }
$ErrorActionPreference = "Continue"
& $iscc /Qp "/DVersion=$version" "/DDist=..\$dist" scripts\talk.iss
exit $LASTEXITCODE
