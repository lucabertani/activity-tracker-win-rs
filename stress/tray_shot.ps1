param([string]$OutFile)
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$tb = $b.Height - [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea.Height
$x = $b.Right - 700
$y = $b.Bottom - $tb
$w = 700
$h = [Math]::Max($tb, 40)
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($x, $y, 0, 0, (New-Object System.Drawing.Size($w, $h)))
$bmp.Save($OutFile, [System.Drawing.Imaging.ImageFormat]::Png)
$red = 0
$minX = 9999; $maxX = -1; $minY = 9999; $maxY = -1
for ($i = 0; $i -lt $w; $i++) {
    for ($j = 0; $j -lt $h; $j++) {
        $c = $bmp.GetPixel($i, $j)
        if ($c.R -gt 150 -and $c.G -lt 100 -and $c.B -lt 100) {
            $red++
            if ($i -lt $minX) { $minX = $i }
            if ($i -gt $maxX) { $maxX = $i }
            if ($j -lt $minY) { $minY = $j }
            if ($j -gt $maxY) { $maxY = $j }
        }
    }
}
if ($red -gt 0) { "red_pixels=$red bbox_x=${minX}..${maxX} y=${minY}..${maxY}" } else { "red_pixels=0" }
