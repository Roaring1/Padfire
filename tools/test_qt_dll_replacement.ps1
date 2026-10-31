[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$InstallDir)

$ErrorActionPreference = "Stop"
$gui = Join-Path $InstallDir "Padfire.exe"
$target = Join-Path $InstallDir "_internal\PySide6\Qt6Core.dll"
$backup = "$target.padfire-original"
$replacement = "$target.padfire-replacement"
if (!(Test-Path -LiteralPath $gui)) { throw "GUI executable not found: $gui" }
if (!(Test-Path -LiteralPath $target)) { throw "QtCore DLL not found: $target" }
if ((Test-Path -LiteralPath $backup) -or (Test-Path -LiteralPath $replacement)) {
    throw "Temporary replacement files already exist; refusing to overwrite them."
}
if (Get-Process -Name Padfire -ErrorAction SilentlyContinue) {
    throw "Padfire is already running; refusing to replace a loaded Qt DLL."
}
$originalHash = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
$process = $null
$restored = $false
try {
    Copy-Item -LiteralPath $target -Destination $replacement
    Move-Item -LiteralPath $target -Destination $backup
    Move-Item -LiteralPath $replacement -Destination $target
    if ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -ne $originalHash) {
        throw "Replacement copy is not byte-identical to original QtCore."
    }
    $process = Start-Process -FilePath $gui -ArgumentList "--smoke-ui" -WorkingDirectory $InstallDir -PassThru -WindowStyle Hidden
    if (!$process.WaitForExit(15000)) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        throw "Padfire did not exit after Qt DLL replacement smoke test."
    }
    if ($process.ExitCode -ne 0) { throw "Padfire exited $($process.ExitCode) after QtCore replacement." }
    Write-Host "PASS: replaced Qt6Core.dll in the installed layout and loaded it."
}
finally {
    if ($process -and !$process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $backup) {
        if (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Force }
        Move-Item -LiteralPath $backup -Destination $target
        $restored = $true
    }
    if (Test-Path -LiteralPath $replacement) { Remove-Item -LiteralPath $replacement -Force }
}
if (!$restored) { throw "Original QtCore DLL backup was not restored." }
if ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -ne $originalHash) {
    throw "Restored QtCore DLL hash does not match the original."
}
Write-Host "PASS: original Qt6Core.dll restored byte-for-byte."
