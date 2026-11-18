# Instala o talk no Windows a partir de target\release, como cópia (no Linux é o
# mesmo motivo: recompilar troca o arquivo debaixo do serviço em execução), e
# liga o início no login.
#
#   .\scripts\build-windows.ps1
#   .\scripts\install-windows.ps1
#
# Fica em %LOCALAPPDATA%\Programs\talk. Para desligar o login:
#   & "$env:LOCALAPPDATA\Programs\talk\talk-rs.exe" autostart off

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot)

$dest = "$env:LOCALAPPDATA\Programs\talk"
New-Item -ItemType Directory -Force $dest | Out-Null

# O Windows não deixa sobrescrever um .exe em uso: o serviço (e o trabalhador)
# saem antes da cópia.
Get-Process talk-rs, talk-ui, talk-pill -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
Remove-Item "$env:LOCALAPPDATA\talk\talk-rs.sock" -ErrorAction SilentlyContinue

foreach ($exe in "talk-rs.exe", "talk-start.exe", "talk-ui.exe", "talk-pill.exe") {
    Copy-Item "target\release\$exe" $dest -Force
}

# A única DLL do CUDA que o talk usa vai junto: assim ele roda mesmo onde o
# CUDA não está no PATH (ou numa máquina só com o driver da NVIDIA).
$cuda = [Environment]::GetEnvironmentVariable("CUDA_PATH", "Machine")
if ($cuda) {
    Get-ChildItem "$cuda\bin" -Recurse -Filter "cublas*64_*.dll" | Copy-Item -Destination $dest -Force
}

# Os modelos que faltam (o mesmo lugar e os mesmos arquivos do instalador).
$models = "$env:LOCALAPPDATA\talk\models"
New-Item -ItemType Directory -Force $models | Out-Null
$hf = "https://huggingface.co"
foreach ($m in @(
    @("ggml-silero-v5.1.2.bin", "$hf/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin"),
    @("ggml-large-v3-q5_0.bin", "$hf/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_0.bin"))) {
    if (-not (Test-Path "$models\$($m[0])")) {
        Write-Host "[talk] baixando $($m[0])"
        curl.exe -fL --progress-bar -o "$models\$($m[0]).part" $m[1]
        if ($LASTEXITCODE -ne 0) { throw "não deu para baixar $($m[0])" }
        Move-Item "$models\$($m[0]).part" "$models\$($m[0])" -Force
    }
}

& "$dest\talk-rs.exe" autostart on
& "$dest\talk-start.exe"
Start-Sleep 2
& "$dest\talk-rs.exe" status
