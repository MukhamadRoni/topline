# Tangkapan layar area tengah-atas monitor utama (tempat notch) -> .tools\out\<nama>.png
param([string]$Name = 'shot', [int]$W = 760, [int]$H = 560)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace D -Name P -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();'
[void][D.P]::SetProcessDPIAware()
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$x = $b.X + [int](($b.Width - $W) / 2)
$bmp = New-Object System.Drawing.Bitmap $W, $H
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($x, $b.Y, 0, 0, (New-Object System.Drawing.Size $W, $H), [System.Drawing.CopyPixelOperation]::SourceCopy)
$bmp.Save("$PSScriptRoot\out\$Name.png", [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output "screen $($b.Width)x$($b.Height) captured $W x $H"
