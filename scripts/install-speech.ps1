# A fala no Windows (`talk-rs speak`): o Qwen3-TTS só roda em Python, num
# ambiente próprio em %LOCALAPPDATA%\talk\speech\venv. O mesmo passo 6b do
# install.sh. Chamado pelo install.ps1 com $env:TALK_SPEECH=1 (ou quando a fala
# já está instalada); dá para rodar sozinho.
#
# As versões vão fixas: o transformers 5.24 quebra o modelo. O PyTorch vem do
# índice da NVIDIA certo para o driver (`--torch-backend auto` do uv): o do
# PyPI no Windows é só CPU.

$ErrorActionPreference = "Stop"
function Say($t) { Write-Host "[talk] $t" -ForegroundColor Cyan }

$speech = "$env:LOCALAPPDATA\talk\speech"
$py = "$speech\venv\Scripts\python.exe"

$uv = (Get-Command uv -ErrorAction SilentlyContinue).Source
if (-not $uv) { $uv = "$env:USERPROFILE\.local\bin\uv.exe" }
if (-not (Test-Path $uv)) {
    Say "instalando o uv (o Python da fala)"
    powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex"
    if (-not (Test-Path $uv)) { throw "o uv não foi instalado" }
}

New-Item -ItemType Directory -Force $speech | Out-Null
# O uv escreve o progresso no stderr; com "Stop" o PowerShell 5 o pinta de erro.
$ErrorActionPreference = "Continue"
if (-not (Test-Path $py)) {
    & $uv venv -q -p 3.12 "$speech\venv"
    if ($LASTEXITCODE -ne 0) { throw "o uv não criou o ambiente da fala" }
}
Say "ambiente da fala (faster-qwen3-tts; a primeira vez baixa o PyTorch com CUDA, ~3 GB)"
& $uv pip install -q -p $py --torch-backend auto torch "faster-qwen3-tts==0.5.4" "transformers==5.15.1"
if ($LASTEXITCODE -ne 0) { throw "o uv não instalou a fala" }
Say "baixando a voz (Qwen3-TTS 0.6B)"
& $py -c "from huggingface_hub import snapshot_download as d; d('Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice')" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "não deu para baixar a voz" }
& $py -c "import torch; assert torch.cuda.is_available(), 'o PyTorch não vê a placa'"
if ($LASTEXITCODE -ne 0) { throw "o PyTorch da fala não vê a placa" }
$ErrorActionPreference = "Stop"
Say "fala pronta: talk-rs speak `"Olá`""
