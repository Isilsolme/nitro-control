$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
    if (-not $cargo) { $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }
    & $cargo build --release
    New-Item -ItemType Directory -Force -Path (Join-Path $root 'dist') | Out-Null

    $pf86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
    $candidates = @(
        (Get-Command makensis.exe -ErrorAction SilentlyContinue).Source,
        (Join-Path $env:LOCALAPPDATA 'tauri\NSIS\makensis.exe'),
        (Join-Path $pf86 'NSIS\makensis.exe'),
        (Join-Path $env:ProgramFiles 'NSIS\makensis.exe')
    )
    $makensis = $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $makensis) {
        throw "makensis not found. Install NSIS or the Tauri NSIS bundle."
    }
    Push-Location (Join-Path $root 'installer')
    try {
        & $makensis 'NitroControl.nsi'
        if ($LASTEXITCODE -ne 0) { throw "makensis failed: $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
    Get-Item (Join-Path $root 'dist\NitroControl-Setup-0.1.1.exe') | Select-Object FullName,Length
} finally {
    Pop-Location
}
