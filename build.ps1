# Gera a versão do Conver para distribuir:
#   dist\Conver-<versão>-portatil.zip   (é só extrair e usar; as configurações ficam na própria pasta)
#   dist\Conver-<versão>-instalador.exe (instala para o usuário atual, com atalhos e menu do botão direito)
#
# Precisa de: Rust, ffmpeg.exe e yt-dlp.exe (em Conver\bin ou no PATH) e Inno Setup 6 para o instalador.
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
foreach ($tool in "ffmpeg", "yt-dlp") {
    if (-not (Test-Path "Conver\bin\$tool.exe")) {
        $found = Get-Command "$tool.exe" -ErrorAction SilentlyContinue
        if (-not $found) { throw "coloque o $tool.exe em Conver\bin (ou no PATH)" }
        Copy-Item $found.Source "Conver\bin\$tool.exe"
    }
}
Copy-Item LICENSE Conver\licencas\Conver-LICENSE.txt -Force
Copy-Item THIRD-PARTY.md Conver\licencas\TERCEIROS.md -Force
Copy-Item licenses\* Conver\licencas -Force -Recurse
Remove-Item Conver\portatil.txt, Conver\dados -Recurse -Force -ErrorAction SilentlyContinue

# 3. versão portátil
New-Item -ItemType Directory -Force dist | Out-Null
$zip = "dist\Conver-$versao-portatil.zip"
$tmp = Join-Path $env:TEMP "conver-portatil"
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$tmp\Conver" | Out-Null
Copy-Item Conver\* "$tmp\Conver" -Recurse
Set-Content -Encoding utf8 "$tmp\Conver\portatil.txt" "Versão portátil do Conver: as configurações e o histórico ficam na pasta 'dados', ao lado do Conver.exe.`r`nApague este arquivo para usar a pasta do Windows (%APPDATA%\Conver)."
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$tmp\Conver" -DestinationPath $zip -CompressionLevel Optimal
Remove-Item $tmp -Recurse -Force

# 4. instalador
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($iscc) {
    & $iscc /Q "/DVersao=$versao" "/DOrigem=$PSScriptRoot\Conver" "/DRepo=https://github.com/$Repo" installer\conver.iss
    if ($LASTEXITCODE -ne 0) { throw "falha ao gerar o instalador" }
} else {
    Write-Warning "Inno Setup 6 não encontrado: o instalador não foi gerado."
}

# 5. somas de verificação
Get-ChildItem dist -File | Where-Object { $_.Name -like "Conver-$versao-*" -and $_.Extension -ne ".txt" } | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content -Encoding ascii "dist\Conver-$versao-SHA256.txt"

Get-ChildItem dist -File | Where-Object Name -like "Conver-$versao-*" | ForEach-Object { "{0,-40} {1,8:N1} MB" -f $_.Name, ($_.Length / 1MB) }
