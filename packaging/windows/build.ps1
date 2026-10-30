[CmdletBinding()]
param(
    [switch]$Installer,
    [switch]$Clean
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Set-Location $RepoRoot

if ($Clean) {
    $BuildDir = Join-Path $RepoRoot "build"
    $DistDir = Join-Path $RepoRoot "dist"
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $BuildDir, $DistDir
}

$Python = (Get-Command python -ErrorAction Stop).Source
& $Python --version
& $Python -m pip install --upgrade pip
& $Python -m pip install -r "packaging/windows/requirements.txt"
& $Python -m compileall -q "padfire_core.py" "padfire_win" "padfire_windows.py"
& $Python -m unittest discover -s tests -p "test_padfire_platform.py"
$env:QT_QPA_PLATFORM = "offscreen"
& $Python -m unittest discover -s tests -p "test_padfire_windows_ui.py"
& $Python "padfire_windows.py" --smoke-ui

& $Python -m PyInstaller --clean --noconfirm "packaging/windows/Padfire.spec"
& $Python -m PyInstaller --clean --noconfirm "packaging/windows/PadfireCLI.spec"
$Executable = Join-Path $RepoRoot "dist\Padfire\Padfire.exe"
$CliExecutable = Join-Path $RepoRoot "dist\PadfireCLI\PadfireCLI.exe"
if (-not (Test-Path $Executable)) {
    throw "PyInstaller did not produce dist\Padfire\Padfire.exe"
}
if (-not (Test-Path $CliExecutable)) {
    throw "PyInstaller did not produce dist\PadfireCLI\PadfireCLI.exe"
}
& $CliExecutable --version
if ($LASTEXITCODE -ne 0) {
    throw "Packaged PadfireCLI --version smoke test failed."
}
$status = & $CliExecutable --status | ConvertFrom-Json
if ($status.running -ne $false) {
    throw "Packaged PadfireCLI --status did not report the expected not-running state."
}

if ($Installer) {
    $Compiler = Get-Command iscc -ErrorAction SilentlyContinue
    if (-not $Compiler) {
        throw "Inno Setup compiler (iscc.exe) is required for -Installer."
    }
    & $Compiler.Source "packaging/windows/Padfire.iss"
}
Write-Host "Windows GUI build completed: $Executable"
Write-Host "Windows control CLI completed: $CliExecutable"
