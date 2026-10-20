# Jalankan dist\topline.exe terlepas dari runner (via WMI, bukan proses turunan).
$ErrorActionPreference = 'Continue'
Get-Process topline -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
$exe = (Resolve-Path "$PSScriptRoot\..\dist\topline.exe").Path
$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = "`"$exe`""; CurrentDirectory = (Split-Path $exe) }
Write-Output "create: $($r.ReturnValue) pid=$($r.ProcessId)"
Start-Sleep -Seconds 4
$p = Get-Process -Id $r.ProcessId -ErrorAction SilentlyContinue
if ($p) { Write-Output ("running  RAM {0:N1} MB  CPU {1:N2}s  handles {2}" -f ($p.WorkingSet64/1MB), $p.CPU, $p.HandleCount) } else { Write-Output "PROSES BERHENTI" }
