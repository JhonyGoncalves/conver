# Gera a versão do Conver para distribuir:
#   dist\Conver-<versão>-portatil.zip   (é só extrair e usar; as configurações ficam na própria pasta)
#   dist\Conver-<versão>-instalador.exe (instala para o usuário atual, com atalhos e menu do botão direito)
#
# Precisa de: Rust, ffmpeg.exe e yt-dlp.exe (em Conver\bin ou no PATH), onnxruntime.dll (Conver\bin),
# silueta.onnx (Conver\modelos) e Inno Setup 6 para o instalador.
# Uso: powershell -ExecutionPolicy Bypass -File build.ps1 [-Repo dono/conver]
param([string]$Repo = "JhonyGoncalves/conver")

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$versao = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
Write-Host "Conver $versao ($Repo)"

# 1. programa
$env:CONVER_REPO = $Repo
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "falha na compilação" }
$target = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory

# 2. pasta Conver\ com o programa, as ferramentas e as licenças
New-Item -ItemType Directory -Force Conver\bin, Conver\licencas | Out-Null
Copy-Item "$target\release\conver.exe" Conver\Conver.exe -Force
# linha de comando e servidor MCP (para scripts e agentes de IA)
Copy-Item "$target\release\conver-cli.exe" Conver\conver-cli.exe -Force
foreach ($tool in "ffmpeg", "yt-dlp") {
    if (-not (Test-Path "Conver\bin\$tool.exe")) {
        $found = Get-Command "$tool.exe" -ErrorAction SilentlyContinue
        if (-not $found) { throw "coloque o $tool.exe em Conver\bin (ou no PATH)" }
        Copy-Item $found.Source "Conver\bin\$tool.exe"
    }
}
# removedor de fundo: ONNX Runtime e o modelo silueta (rembg)
$extras = @{
    "Conver\bin\onnxruntime.dll"  = "https://github.com/microsoft/onnxruntime/releases/download/v1.22.0/onnxruntime-win-x64-1.22.0.zip (lib\onnxruntime.dll)"
    "Conver\modelos\silueta.onnx" = "https://github.com/danielgatis/rembg/releases/download/v0.0.0/silueta.onnx"
    # desfocar rostos: detector YuNet (OpenCV Zoo, MIT)
    "Conver\modelos\face_detection_yunet_2023mar.onnx" = "https://huggingface.co/opencv/face_detection_yunet/resolve/3cc26e7f1014a5ee5d74a42acee58bafc9d0a310/face_detection_yunet_2023mar.onnx"
    # remover ruído: modelo do RNNoise (GregorR/rnnoise-models, somnolent-hogwash, sem direitos autorais)
    "Conver\modelos\rnnoise-sh.rnnn" = "https://raw.githubusercontent.com/GregorR/rnnoise-models/3eee541a283fd3b8f81b85b1748e3b9ccbefa04d/somnolent-hogwash-2018-09-01/sh.rnnn"
    # ampliar com IA: realesr-general-x4v3.pth oficial (xinntao/Real-ESRGAN v0.2.5.0) exportado para ONNX
    "Conver\modelos\realesr-general-x4v3.onnx" = "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesr-general-x4v3.pth (exportar para ONNX)"
    # transcrever: whisper-cli.exe e DLLs do whisper-bin-x64.zip (os modelos são baixados pelo app)
    "Conver\bin\whisper\whisper-cli.exe" = "https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-x64.zip (whisper-cli.exe, whisper.dll, ggml*.dll)"
    # ditado ao vivo: whisper-server.exe do mesmo pacote
    "Conver\bin\whisper\whisper-server.exe" = "https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-x64.zip (Release\whisper-server.exe)"
    # texto para voz: pasta piper do piper_windows_amd64.zip (as vozes são baixadas pelo app)
    "Conver\bin\piper\piper.exe" = "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip (pasta piper, sem pkgconfig)"
    # IA local de linguagem: llama-server do llama.cpp com Vulkan (o modelo Qwen é baixado pelo app)
    # gerar imagem: stable-diffusion.cpp com Vulkan (o modelo é baixado pelo app)
    "Conver\bin\sd\sd-cli.exe" = "https://github.com/leejet/stable-diffusion.cpp/releases/download/master-1050-58bb3c1/sd-master-58bb3c1-bin-win-vulkan-x64.zip (sem sd-server.exe e start-server.*)"
}
foreach ($f in $extras.Keys) {
    if (-not (Test-Path $f)) { throw "falta $f - baixe de $($extras[$f])" }
}
Copy-Item LICENSE Conver\licencas\Conver-LICENSE.txt -Force
Copy-Item THIRD-PARTY.md Conver\licencas\TERCEIROS.md -Force
Copy-Item licenses\* Conver\licencas -Force -Recurse
Remove-Item Conver\portatil.txt, Conver\dados -Recurse -Force -ErrorAction SilentlyContinue

# o que vai no download: a pasta Conver sem o llama.cpp (bin\llm), que o app baixa só para quem usa a IA de
# linguagem (llm::ensure_runtime); a cópia em Conver\bin\llm fica só para o desenvolvimento e os testes
$stage = Join-Path $env:TEMP "conver-pacote"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage\Conver" | Out-Null
Copy-Item Conver\* "$stage\Conver" -Recurse
Remove-Item "$stage\Conver\bin\llm" -Recurse -Force -ErrorAction SilentlyContinue

# 3. versão portátil
New-Item -ItemType Directory -Force dist | Out-Null
$zip = "dist\Conver-$versao-portatil.zip"
$tmp = Join-Path $env:TEMP "conver-portatil"
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$tmp\Conver" | Out-Null
Copy-Item "$stage\Conver\*" "$tmp\Conver" -Recurse
Set-Content -Encoding utf8 "$tmp\Conver\portatil.txt" "Versão portátil do Conver: as configurações e o histórico ficam na pasta 'dados', ao lado do Conver.exe.`r`nApague este arquivo para usar a pasta do Windows (%APPDATA%\Conver)."
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$tmp\Conver" -DestinationPath $zip -CompressionLevel Optimal
Remove-Item $tmp -Recurse -Force

# 4. instalador
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($iscc) {
    & $iscc /Q "/DVersao=$versao" "/DOrigem=$stage\Conver" "/DRepo=https://github.com/$Repo" installer\conver.iss
    if ($LASTEXITCODE -ne 0) { throw "falha ao gerar o instalador" }
} else {
    Write-Warning "Inno Setup 6 não encontrado: o instalador não foi gerado."
}

Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue

# 5. somas de verificação
Get-ChildItem dist -File | Where-Object { $_.Name -like "Conver-$versao-*" -and $_.Extension -ne ".txt" } | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content -Encoding ascii "dist\Conver-$versao-SHA256.txt"

Get-ChildItem dist -File | Where-Object Name -like "Conver-$versao-*" | ForEach-Object { "{0,-40} {1,8:N1} MB" -f $_.Name, ($_.Length / 1MB) }
