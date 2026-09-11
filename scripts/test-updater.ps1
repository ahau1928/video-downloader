$ErrorActionPreference='Stop'
$scriptPath=Join-Path $PSScriptRoot 'fetch-tools.ps1'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($scriptPath,[ref]$tokens,[ref]$errors)
if($errors.Count){throw ($errors | Out-String)}
# Exercise the production checksum functions without invoking network or tool installation.
foreach($name in @('Hash','CheckHash')){
    $definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$dir=Join-Path (Split-Path $PSScriptRoot -Parent) '.review/updater-test'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$file=Join-Path $dir 'payload.bin'
[IO.File]::WriteAllText($file,'verified component')
$expected=Hash $file
CheckHash $file $expected
[IO.File]::WriteAllText($file,'corrupted component')
$rejected=$false
try{CheckHash $file $expected}catch{$rejected=$true}
if(!$rejected){throw 'Corrupt payload was accepted'}
'PASS: matching SHA256 accepted; corrupted payload rejected; updater PowerShell syntax valid'
