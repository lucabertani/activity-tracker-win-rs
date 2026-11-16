# Regenerate the app icon with built-in Windows/.NET drawing APIs.
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File assets/generate-icon.ps1
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

function New-RoundedRectangle([single]$X, [single]$Y, [single]$Width, [single]$Height, [single]$Radius) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $diameter = 2 * $Radius
    $path.AddArc($X, $Y, $diameter, $diameter, 180, 90)
    $path.AddArc(($X + $Width - $diameter), $Y, $diameter, $diameter, 270, 90)
    $path.AddArc(($X + $Width - $diameter), ($Y + $Height - $diameter), $diameter, $diameter, 0, 90)
    $path.AddArc($X, ($Y + $Height - $diameter), $diameter, $diameter, 90, 90)
    $path.CloseFigure()
    return ,$path
}

function New-IconBitmap([int]$Size) {
    # Supersample each size independently to keep the small frames crisp.
    $canvas = New-Object System.Drawing.Bitmap ($Size * 4), ($Size * 4)
    $graphics = [System.Drawing.Graphics]::FromImage($canvas)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.ScaleTransform(($Size / 64.0), ($Size / 64.0))

    $badge = New-RoundedRectangle 8 8 240 240 52
    $monitor = New-RoundedRectangle 44 54 168 122 16
    $gradient = New-Object System.Drawing.Drawing2D.LinearGradientBrush (
        [System.Drawing.Point]::new(32, 8)), (
        [System.Drawing.Point]::new(224, 248)), (
        [System.Drawing.ColorTranslator]::FromHtml('#193C52')), (
        [System.Drawing.ColorTranslator]::FromHtml('#0B1827'))
    $rim = New-Object System.Drawing.Pen ([System.Drawing.ColorTranslator]::FromHtml('#355D73')), 3
    $screen = New-Object System.Drawing.Pen ([System.Drawing.ColorTranslator]::FromHtml('#71DAF4')), 12
    $pulse = New-Object System.Drawing.Pen ([System.Drawing.ColorTranslator]::FromHtml('#C4EF83')), 11
    $screen.StartCap = $screen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pulse.StartCap = $pulse.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pulse.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    try {
        $graphics.FillPath($gradient, $badge)
        $graphics.DrawPath($rim, $badge)
        $graphics.DrawPath($screen, $monitor)
        $graphics.DrawLine($screen, 128, 178, 128, 204)
        $graphics.DrawLine($screen, 96, 204, 160, 204)
        $points = [System.Drawing.PointF[]]@(
            [System.Drawing.PointF]::new(62, 124),
            [System.Drawing.PointF]::new(86, 124),
            [System.Drawing.PointF]::new(107, 91),
            [System.Drawing.PointF]::new(132, 144),
            [System.Drawing.PointF]::new(152, 113),
            [System.Drawing.PointF]::new(194, 113)
        )
        $graphics.DrawLines($pulse, $points)
        $bitmap = New-Object System.Drawing.Bitmap $Size, $Size
        $output = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $output.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $output.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
            $output.DrawImage($canvas, 0, 0, $Size, $Size)
        } finally {
            $output.Dispose()
        }
        return ,$bitmap
    } finally {
        $graphics.Dispose()
        $canvas.Dispose()
        $badge.Dispose()
        $monitor.Dispose()
        $gradient.Dispose()
        $rim.Dispose()
        $screen.Dispose()
        $pulse.Dispose()
    }
}

$sizes = @(16, 20, 24, 32, 40, 48, 64, 96, 128, 256)
$frames = @()
foreach ($size in $sizes) {
    $bitmap = New-IconBitmap $size
    $stream = New-Object System.IO.MemoryStream
    try {
        $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
        $frames += ,$stream.ToArray()
        if ($size -eq 256) {
            $bitmap.Save((Join-Path $PSScriptRoot 'activity-tracker.png'), [System.Drawing.Imaging.ImageFormat]::Png)
        }
    } finally {
        $stream.Dispose()
        $bitmap.Dispose()
    }
}

# ICO directory followed by PNG-compressed frames (Windows Vista and later).
$icon = [System.IO.File]::Create((Join-Path $PSScriptRoot 'activity-tracker.ico'))
$writer = New-Object System.IO.BinaryWriter $icon
try {
    $writer.Write([uint16]0)
    $writer.Write([uint16]1)
    $writer.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($index = 0; $index -lt $sizes.Count; $index++) {
        $dimension = $sizes[$index] % 256 # 0 represents 256 px.
        $writer.Write([byte]$dimension)
        $writer.Write([byte]$dimension)
        $writer.Write([byte]0)
        $writer.Write([byte]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]32)
        $writer.Write([uint32]$frames[$index].Length)
        $writer.Write([uint32]$offset)
        $offset += $frames[$index].Length
    }
    foreach ($frame in $frames) {
        $writer.Write([byte[]]$frame)
    }
} finally {
    $writer.Dispose()
}
Write-Output "Generated activity-tracker.ico ($($sizes -join ', ') px) and activity-tracker.png"
