$p = Get-Process topline -ErrorAction SilentlyContinue
if ($p) { Write-Output ("RAM {0:N1} MB  private {1:N1} MB  CPU {2:N2}s" -f ($p.WorkingSet64/1MB), ($p.PrivateMemorySize64/1MB), $p.CPU) } else { Write-Output "tidak berjalan" }
