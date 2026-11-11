# Compila o talk no Windows, com o ambiente que o whisper.cpp com CUDA precisa.
#
#   .\scripts\build-windows.ps1                  # cargo build --release -p talk
#   .\scripts\build-windows.ps1 test -p talk     # qualquer outro comando do cargo
#
# Precisa de: Rust (rustup), Visual Studio 2022 com C++, CUDA Toolkit >= 12.8
# (RTX 50xx), LLVM e CMake. CUDA_ARCH muda a placa: 120 = RTX 50xx, 89 = RTX 40xx,
# 86 = RTX 30xx.

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot)

# Variáveis que um instalador criou depois que este terminal abriu (CUDA_PATH,
# CUDA_PATH_V13_5...): o MSBuild do CUDA falha sem elas.
foreach ($scope in "Machine", "User") {
    $vars = [Environment]::GetEnvironmentVariables($scope)
    foreach ($k in $vars.Keys) {
        if ($k -ne "Path" -and -not (Test-Path "env:$k")) { Set-Item "env:$k" $vars[$k] }
    }
}
$env:Path = "$env:USERPROFILE\.cargo\bin;" + [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [Environment]::GetEnvironmentVariable("Path", "User")

# O ambiente de desenvolvedor do Visual Studio: sem o INCLUDE dele, o bindgen
# (libclang) não acha o stdio.h e o whisper-rs cai nos bindings do Linux.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw "Visual Studio com as ferramentas de C++ não encontrado" }
cmd /c "`"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && set" | ForEach-Object {
    if ($_ -match "^([^=]+)=(.*)$") { Set-Item "env:$($Matches[1])" $Matches[2] }
}

if (-not $env:LIBCLANG_PATH) { $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin" }
$env:CMAKE_CUDA_ARCHITECTURES = if ($env:CUDA_ARCH) { $env:CUDA_ARCH } else { "120" }

if ($args.Count -eq 0) { $args = @("build", "--release", "-p", "talk") }
# O cargo escreve o progresso no stderr, e o PowerShell 5 com "Stop" trata
# qualquer linha de stderr de um programa como erro fatal.
$ErrorActionPreference = "Continue"
cargo @args
exit $LASTEXITCODE
