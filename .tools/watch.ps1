# Topline build runner: menjalankan skrip di .tools\req\*.ps1, hasil ke .tools\out\.
# Tutup jendela ini untuk menghentikan.
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$Host.UI.RawUI.WindowTitle = 'Topline build runner (jangan klik di dalam jendela ini)'
# Matikan QuickEdit agar klik di jendela tidak membekukan proses.
try {
  Add-Type -Namespace W -Name K -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern System.IntPtr GetStdHandle(int h);
[DllImport("kernel32.dll")] public static extern bool GetConsoleMode(System.IntPtr h, out uint m);
[DllImport("kernel32.dll")] public static extern bool SetConsoleMode(System.IntPtr h, uint m);
'@
  $h = [W.K]::GetStdHandle(-10); $m = 0
  if ([W.K]::GetConsoleMode($h, [ref]$m)) { [void][W.K]::SetConsoleMode($h, ($m -band (-bnot 0x40)) -bor 0x80) }
} catch {}
Write-Host 'Topline build runner aktif di' $root -ForegroundColor Cyan
Write-Host 'Biarkan jendela ini terbuka (boleh diminimalkan). Tutup untuk berhenti.'
$done = @{}
Get-ChildItem "$root\.tools\out\*.done" -ErrorAction SilentlyContinue | ForEach-Object { $done[$_.BaseName] = $true }
$beat = 0
while ($true) {
  try {
    Get-ChildItem "$root\.tools\req" -Filter *.ps1 -ErrorAction SilentlyContinue | Sort-Object Name | ForEach-Object {
      $id = $_.BaseName
      if (-not $done.ContainsKey($id)) {
        $done[$id] = $true
        $log = "$root\.tools\out\$id.log"
        Write-Host "[$(Get-Date -Format HH:mm:ss)] menjalankan $id ..."
        $p = Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"`"$($_.FullName)`"" -RedirectStandardOutput $log -RedirectStandardError "$log.err" -NoNewWindow -PassThru -Wait
        "EXIT:$($p.ExitCode)" | Out-File -Encoding utf8 "$root\.tools\out\$id.done"
        Write-Host "[$(Get-Date -Format HH:mm:ss)] selesai $id (exit $($p.ExitCode))"
      }
    }
  } catch { Write-Host "error: $_" -ForegroundColor Red }
  $beat++
  if ($beat % 12 -eq 0) { (Get-Date -Format o) | Out-File -Encoding utf8 "$root\.tools\heartbeat.txt" }
  Start-Sleep -Milliseconds 800
}
