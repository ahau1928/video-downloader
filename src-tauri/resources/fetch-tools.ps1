param([Parameter(Mandatory=$true)][string]$Destination, [ValidateSet('all','yt-dlp','ffmpeg','deno')][string]$Only='all', [string]$Proxy='', [switch]$CheckOnly)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12
$root=[IO.Path]::GetFullPath($Destination)
New-Item -ItemType Directory -Force -Path $root | Out-Null
function Request([string]$url, [string]$out='') {
    $arguments=@('-f','-sS','-L','--retry','3','--retry-all-errors','--connect-timeout','20','--max-time','600','--proto','=https','-A','VideoDownloader/0.2.0')
    if($Proxy){$arguments+=@('--proxy',$Proxy)}
    if($out){$arguments+=@('-o',$out)}
    $response=& curl.exe @arguments $url
    if($LASTEXITCODE -ne 0){throw "Download failed: $url"}
    if(!$out){return ($response -join "`n")}
}
function Hash([string]$path){(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
function CheckHash([string]$path,[string]$expected){if((Hash $path) -ne $expected.ToLowerInvariant()){throw "SHA256 verification failed: $([IO.Path]::GetFileName($path))"}}
function GetRelease([string]$repo){
    # Public release pages also work when the shared GitHub API quota is exhausted.
    $html=Request "https://github.com/$repo/releases/latest"
    $pattern='/'+[regex]::Escape($repo)+'/releases/tag/([^"<>?\s]+)'
    $match=[regex]::Match($html,$pattern)
    if(!$match.Success){throw "Cannot identify latest release: $repo"}
    $tag=[Net.WebUtility]::HtmlDecode($match.Groups[1].Value)
    if($tag -notmatch '^[a-zA-Z0-9._-]+$'){throw 'Invalid release tag'}
    $assetHtml=Request "https://github.com/$repo/releases/expanded_assets/$tag"
    $assets=@()
    foreach($li in [regex]::Matches($assetHtml,'(?s)<li\b.*?</li>')){
        $link=[regex]::Match($li.Value,'href="(/'+[regex]::Escape($repo)+'/releases/download/'+[regex]::Escape($tag)+'/([^"/]+))"')
        $hash=[regex]::Match($li.Value,'sha256:([a-fA-F0-9]{64})')
        if($link.Success -and $hash.Success){$assets+= [pscustomobject]@{name=$link.Groups[2].Value;browser_download_url=('https://github.com'+$link.Groups[1].Value);digest=('sha256:'+$hash.Groups[1].Value)}}
    }
    return [pscustomobject]@{tag_name=$tag;assets=$assets}
}
function Asset($release,[string]$name){$a=$release.assets | Where-Object name -eq $name | Select-Object -First 1;if(!$a){throw "Missing release asset: $name"};return $a}
function Digest($asset){if($asset.digest -match '^sha256:([a-fA-F0-9]{64})$'){return $Matches[1]};throw "Release does not publish a SHA256 digest: $($asset.name)"}
$activeFile=Join-Path $root 'active.json'
$active=@{}
if(Test-Path -LiteralPath $activeFile){$old=Get-Content -LiteralPath $activeFile -Raw | ConvertFrom-Json;foreach($p in $old.PSObject.Properties){$active[$p.Name]=$p.Value}}
$results=@()
foreach($tool in @('yt-dlp','ffmpeg','deno')){
    if($Only -ne 'all' -and $Only -ne $tool){continue}
    if($tool -eq 'ffmpeg'){
        $version=(Request 'https://www.gyan.dev/ffmpeg/builds/release-version').Trim()
        if($version -notmatch '^\d+\.\d+(\.\d+)?$'){throw 'Unexpected FFmpeg version response'}
        $url="https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-$version-essentials_build.zip"
        $expected=((Request "$url.sha256").Trim() -split '\s+')[0]
        if($expected -notmatch '^[a-fA-F0-9]{64}$'){throw 'Invalid FFmpeg checksum'}
    }else{
        $repo=if($tool -eq 'deno'){'denoland/deno'}else{'yt-dlp/yt-dlp'}
        $release=GetRelease $repo
        $version=$release.tag_name
        $name=if($tool -eq 'deno'){'deno-x86_64-pc-windows-msvc.zip'}else{'yt-dlp.exe'}
        $asset=Asset $release $name
        $url=$asset.browser_download_url
        $expected=Digest $asset
    }
    $current=if($active.ContainsKey($tool)){$active[$tool].version}else{''}
    $result=[ordered]@{name=$tool;version=$version;current=$current;updated=$false;available=($current -ne $version)}
    if(!$CheckOnly -and $current -ne $version){
        $safeVersion=$version -replace '[^a-zA-Z0-9._-]','_'
        $dirName="$tool-$safeVersion-$([guid]::NewGuid().ToString('N').Substring(0,8))"
        $stage=Join-Path $root $dirName
        $cached=Get-ChildItem -LiteralPath $root -Directory | Where-Object {$_.Name.StartsWith("$tool-$safeVersion-")} | Where-Object { $archive=Join-Path $_.FullName $(if($tool -eq 'yt-dlp'){'yt-dlp.exe'}else{'package.zip'}); (Test-Path -LiteralPath $archive) -and (Hash $archive) -eq $expected.ToLowerInvariant() } | Select-Object -First 1
        if($cached){$stage=$cached.FullName;$dirName=$cached.Name}
        New-Item -ItemType Directory -Force -Path $stage | Out-Null
        $download=Join-Path $stage $(if($tool -eq 'yt-dlp'){'yt-dlp.exe'}else{'package.zip'})
        if(!$cached){Request $url $download}
        CheckHash $download $expected
        if($tool -ne 'yt-dlp'){
            Expand-Archive -LiteralPath $download -DestinationPath (Join-Path $stage 'unpacked') -Force
            $names=if($tool -eq 'ffmpeg'){@('ffmpeg.exe','ffprobe.exe')}else{@('deno.exe')}
            foreach($exe in $names){
                $source=Get-ChildItem -LiteralPath (Join-Path $stage 'unpacked') -Recurse -File -Filter $exe | Select-Object -First 1
                if(!$source){throw "Missing executable: $exe"}
                Copy-Item -LiteralPath $source.FullName -Destination (Join-Path $stage $exe)
            }
            # Keep upstream licenses alongside the executable.
            Get-ChildItem -LiteralPath (Join-Path $stage 'unpacked') -Recurse -File | Where-Object {$_.Name -match '^(LICENSE|COPYING)'} | ForEach-Object {Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $stage $_.Name) -Force}
        }
        $exeName=if($tool -eq 'ffmpeg'){'ffmpeg'}else{$tool}
        $arg=if($tool -eq 'ffmpeg'){'-version'}else{'--version'}
        $versionOutput=& (Join-Path $stage "$exeName.exe") $arg 2>&1
        if($LASTEXITCODE -ne 0){throw "New $tool failed to start"}
        if($tool -eq 'ffmpeg'){& (Join-Path $stage 'ffprobe.exe') -version 2>&1 | Out-Null;if($LASTEXITCODE -ne 0){throw 'New ffprobe failed to start'}}
        $active[$tool]=[ordered]@{version=$version;directory=$dirName;source=$url;sha256=$expected;display=($versionOutput | Select-Object -First 1).ToString()}
        $result.updated=$true
    }
    $results+=$result
}
if(!$CheckOnly -and ($results | Where-Object updated)){
    $json=$active | ConvertTo-Json -Depth 8
    $temp=Join-Path $root 'active.next.json'
    [IO.File]::WriteAllText($temp,$json,(New-Object Text.UTF8Encoding($false)))
    if(Test-Path -LiteralPath $activeFile){[IO.File]::Replace($temp,$activeFile,(Join-Path $root 'previous.json'),$true)}else{[IO.File]::Move($temp,$activeFile)}
}
ConvertTo-Json -InputObject @($results) -Depth 8 -Compress
