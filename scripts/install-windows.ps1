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

& "$dest\talk-rs.exe" autostart on
& "$dest\talk-start.exe"
Start-Sleep 2
& "$dest\talk-rs.exe" status
