param([switch]$Refresh,[string]$Proxy='http://127.0.0.1:7897')
$ErrorActionPreference='Stop'
$project=Split-Path $PSScriptRoot -Parent
$store=Join-Path $project 'src-tauri/resources/tool-store'
if($Refresh){& "$PSScriptRoot/fetch-tools.ps1" -Destination $store -Proxy $Proxy;if(!$?){throw 'Tool download failed'}}
$manifest=Get-Content -LiteralPath (Join-Path $store 'active.json') -Raw | ConvertFrom-Json
$bin=Join-Path $project 'src-tauri/resources/bin'
New-Item -ItemType Directory -Force -Path $bin | Out-Null
foreach($name in @('yt-dlp','ffmpeg','ffprobe','deno')){
    $key=if($name -eq 'ffprobe'){'ffmpeg'}else{$name}
    $entry=$manifest.$key
    if(!$entry){throw "Missing manifest: $key"}
    $source=Join-Path (Join-Path $store $entry.directory) "$name.exe"
    Copy-Item -LiteralPath $source -Destination (Join-Path $bin "$name.exe") -Force
    $versionArg=if($name -in @('ffmpeg','ffprobe')){'-version'}else{'--version'}
    $versionText=& (Join-Path $bin "$name.exe") $versionArg
    if($LASTEXITCODE -ne 0){throw "Failed to validate $name"}
    $versionText | Select-Object -First 1
}
Copy-Item -LiteralPath (Join-Path $store 'active.json') -Destination (Join-Path $bin 'bundled-tools.json') -Force
Copy-Item -LiteralPath (Join-Path (Join-Path $store $manifest.ffmpeg.directory) 'LICENSE') -Destination (Join-Path $bin 'LICENSE-FFmpeg.txt') -Force
Copy-Item -LiteralPath "$PSScriptRoot/fetch-tools.ps1" -Destination (Join-Path $project 'src-tauri/resources/fetch-tools.ps1') -Force
Get-ChildItem -LiteralPath $bin -Filter '*.exe' | ForEach-Object {[pscustomobject]@{file=$_.Name;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash}} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $bin 'checksums.json') -Encoding utf8
