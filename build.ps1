# Build Topline + installer di Windows.
# Hasil: dist\setup.exe (installer) dan dist\topline.exe (aplikasi portabel).
# Toolchain Rust dipasang di .toolchain\ (di dalam folder project) jika belum ada.
param([switch]$SkipSetup)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = $PSScriptRoot
Set-Location $root

$env:RUSTUP_HOME = "$root\.toolchain\rustup"
$env:CARGO_HOME  = "$root\.toolchain\cargo"
$env:CARGO_TARGET_DIR = "$root\target"
# Hindari MinGW 32-bit lama di PATH tertukar dengan linker milik Rust.
$env:PATH = "$env:CARGO_HOME\bin;" + ((($env:PATH -split ';') | Where-Object { $_ -and ($_ -notmatch 'MinGW') }) -join ';')

function Step($t) { Write-Output ""; Write-Output "==> $t" }

if (-not (Test-Path "$env:CARGO_HOME\bin\cargo.exe")) {
  Step "Memasang toolchain Rust (stable, x86_64-pc-windows-gnu) ke .toolchain"
  New-Item -ItemType Directory -Force "$root\.toolchain" | Out-Null
  $init = "$root\.toolchain\rustup-init.exe"
  Invoke-WebRequest -UseBasicParsing 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile $init
  cmd /c "`"$init`" -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-gnu --default-toolchain stable 2>&1"
  if ($LASTEXITCODE -ne 0) { throw "rustup-init gagal ($LASTEXITCODE)" }
}
cmd /c "cargo --version 2>&1"
cmd /c "rustc --version 2>&1"

# Font Geist (SIL Open Font License) dari Google Fonts, dibundel ke dalam exe.
$fonts = "$root\assets\fonts"
if (-not (Test-Path "$fonts\Geist-Regular.ttf")) {
  Step "Mengunduh font Geist"
  New-Item -ItemType Directory -Force $fonts | Out-Null
  try {
    $want = @(
      @{ family = 'Geist';      weight = 400; file = 'Geist-Regular.ttf' },
      @{ family = 'Geist';      weight = 500; file = 'Geist-Medium.ttf' },
      @{ family = 'Geist Mono'; weight = 400; file = 'GeistMono-Regular.ttf' }
    )
    foreach ($f in $want) {
      $fam = $f.family -replace ' ', '+'
      $css = (Invoke-WebRequest -UseBasicParsing "https://fonts.googleapis.com/css2?family=$($fam):wght@$($f.weight)" -UserAgent 'Mozilla/4.0').Content
      $m = [regex]::Match($css, 'url\((https://[^)]+\.ttf)\)')
      if ($m.Success) {
        Invoke-WebRequest -UseBasicParsing $m.Groups[1].Value -OutFile "$fonts\$($f.file)"
        Write-Output "  $($f.file)"
      } else { Write-Output "  (tidak menemukan TTF untuk $($f.family) $($f.weight))" }
    }
  } catch { Write-Output "  Gagal mengunduh font, memakai Segoe UI Variable: $_" }
}

# Ikon + manifest exe lewat windres (MinGW-w64), hanya untuk langkah ini.
$windres = @('C:\MinGW\bin\windres.exe', 'C:\msys64\mingw64\bin\windres.exe') | Where-Object { Test-Path $_ } | Select-Object -First 1
$resDir = "$root\target\res"
New-Item -ItemType Directory -Force $resDir | Out-Null
if ($windres) {
  Step "Menyusun resource ikon dengan $windres"
  $oldPath = $env:PATH
  $env:PATH = (Split-Path $windres) + ';' + $env:PATH
  foreach ($r in @(@{ dir = 'apps\topline'; rc = 'topline.rc'; out = 'topline.res.o'; var = 'TOPLINE_APP_RES' },
                   @{ dir = 'apps\topline-setup'; rc = 'setup.rc'; out = 'setup.res.o'; var = 'TOPLINE_SETUP_RES' })) {
    Push-Location "$root\$($r.dir)"
    cmd /c "`"$windres`" --input $($r.rc) --output-format=coff --target=pe-x86-64 --output `"$resDir\$($r.out)`" 2>&1"
    Pop-Location
    if ($LASTEXITCODE -eq 0) { Set-Item "env:$($r.var)" "$resDir\$($r.out)"; Write-Output "  $($r.out)" }
    else { Write-Output "  (gagal membuat $($r.out), exe tanpa ikon)" }
  }
  $env:PATH = $oldPath
} else { Write-Output "windres tidak ditemukan: exe dibuat tanpa ikon tertanam." }

# Tutup Topline versi uji dari folder ini agar dist\topline.exe bisa ditimpa.
Get-Process topline -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$root\*" } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

Step "Build topline.exe"
cmd /c "cargo build --release -p topline 2>&1"
if ($LASTEXITCODE -ne 0) { throw "Build topline gagal" }

New-Item -ItemType Directory -Force "$root\dist" | Out-Null
Copy-Item -Force "$root\target\release\topline.exe" "$root\dist\topline.exe"

if (-not $SkipSetup) {
  Step "Build setup.exe"
  $env:TOPLINE_EXE = "$root\target\release\topline.exe"
  cmd /c "cargo build --release -p topline-setup 2>&1"
  if ($LASTEXITCODE -ne 0) { throw "Build setup gagal" }
  Copy-Item -Force "$root\target\release\setup.exe" "$root\dist\setup.exe"
}

Step "Selesai"
Get-ChildItem "$root\dist" | Select-Object Name, @{n='KB';e={[math]::Round($_.Length/1KB)}} | Format-Table | Out-String | Write-Output
