# Compila res\esme.rc -> res\esme_res.lib (ícono, manifiesto y versión) con las
# herramientas del Windows SDK/MSVC, en lugar de un build.rs (el build queda sin
# ejecutables auxiliares y más rápido).
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$rc = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\rc.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $rc) { throw 'No se encontró rc.exe (Windows SDK).' }
$ver = Split-Path -Leaf (Split-Path -Parent (Split-Path -Parent $rc.FullName))
$kit = "${env:ProgramFiles(x86)}\Windows Kits\10\Include\$ver"
$vs = & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath
$msvc = Get-ChildItem "$vs\VC\Tools\MSVC\*\bin\Hostx64\x64" | Sort-Object FullName -Descending | Select-Object -First 1
Push-Location "$root\res"
try {
    & $rc.FullName /nologo /I "$kit\um" /I "$kit\shared" /fo esme.res esme.rc
    if ($LASTEXITCODE) { throw "rc.exe falló ($LASTEXITCODE)" }
    & "$($msvc.FullName)\cvtres.exe" /nologo /machine:x64 /out:esme_res.obj esme.res
    if ($LASTEXITCODE) { throw "cvtres.exe falló ($LASTEXITCODE)" }
    & "$($msvc.FullName)\lib.exe" /nologo /out:esme_res.lib esme_res.obj
    if ($LASTEXITCODE) { throw "lib.exe falló ($LASTEXITCODE)" }
    Remove-Item esme.res, esme_res.obj
} finally { Pop-Location }
Write-Host "OK: $root\res\esme_res.lib"
