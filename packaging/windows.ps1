param([string]$Bin = "target/release", [string]$Arch = "x86_64")
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$version = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$compiler = @(Get-Command iscc.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source)
$compiler += "$env:LOCALAPPDATA/Programs/Inno Setup 6/ISCC.exe", "${env:ProgramFiles(x86)}/Inno Setup 6/ISCC.exe"
$iscc = $compiler | Where-Object { Test-Path $_ } | Select-Object -First 1
if (!$iscc) { throw "Install Inno Setup 6." }
& $iscc "/DBin=$(Resolve-Path $Bin)" "/DVersion=$version" "/DArch=$Arch" "$PSScriptRoot/windows.iss"
exit $LASTEXITCODE
