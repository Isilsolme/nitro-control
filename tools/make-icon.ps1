Add-Type -AssemblyName System.Drawing

$ErrorActionPreference = 'Stop'
$outIco = Join-Path $PSScriptRoot '..\widget\assets\icon.ico'
$outPng = Join-Path $PSScriptRoot '..\widget\ui\icon.png'
$preview = Join-Path $PSScriptRoot '..\icon-preview.png'

function New-IconBitmap([int]$s) {
    $bmp = [System.Drawing.Bitmap]::new($s, $s, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)

    $margin = if ($s -le 24) { 1.0 } else { [Math]::Max(2.0, $s * 0.07) }
    $tile = $s - 2.0 * $margin
    $radius = [Math]::Max(2.0, $s * 0.20)
    $x = $margin
    $y = $margin
    $d = 2.0 * $radius

    $path = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc($x + $tile - $d, $y, $d, $d, 270, 90)
    $path.AddArc($x + $tile - $d, $y + $tile - $d, $d, $d, 0, 90)
    $path.AddArc($x, $y + $tile - $d, $d, $d, 90, 90)
    $path.CloseFigure()

    $bg = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 11, 15, 21))
    $g.FillPath($bg, $path)
    $borderWidth = [Math]::Max(1.0, $s * 0.012)
    $border = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(255, 46, 230, 197), $borderWidth)
    $g.DrawPath($border, $path)

    if ($s -ge 32) {
        $ringR = $s * 0.30
        $ringW = [Math]::Max(1.4, $s * 0.032)
        $ringPen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(255, 76, 194, 255), $ringW)
        $ringPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
        $ringPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
        $rect = [System.Drawing.RectangleF]::new($s / 2.0 - $ringR, $s / 2.0 - $ringR, $ringR * 2.0, $ringR * 2.0)
        $g.DrawArc($ringPen, $rect, 135, 270)
        if ($s -ge 96) {
            $tail = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(255, 167, 139, 250), $ringW)
            $tail.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
            $tail.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
            $g.DrawArc($tail, $rect, 405, 35)
            $tail.Dispose()
        }
        $ringPen.Dispose()
    }

    $nw = $s * 0.26
    $nh = $s * 0.30
    $cx = $s / 2.0
    $cy = $s / 2.0
    $nPenW = [Math]::Max(1.5, $s * 0.055)
    $nPen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(255, 238, 244, 251), $nPenW)
    $nPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $nPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $left = $cx - $nw / 2.0
    $right = $cx + $nw / 2.0
    $top = $cy - $nh / 2.0
    $bottom = $cy + $nh / 2.0
    $g.DrawLine($nPen, $left, $top, $left, $bottom)
    $g.DrawLine($nPen, $left, $top, $right, $bottom)
    $g.DrawLine($nPen, $right, $top, $right, $bottom)
    $nPen.Dispose()

    if ($s -le 24) {
        $dotR = [Math]::Max(1.5, $s * 0.09)
        $dot = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 46, 230, 197))
        $g.FillEllipse($dot, $cx - $dotR, $s * 0.80 - $dotR, $dotR * 2.0, $dotR * 2.0)
        $dot.Dispose()
    }

    $bg.Dispose()
    $border.Dispose()
    $path.Dispose()
    $g.Dispose()
    return $bmp
}

function Get-DibBytes([System.Drawing.Bitmap]$bmp) {
    $w = $bmp.Width
    $h = $bmp.Height
    $rect = [System.Drawing.Rectangle]::new(0, 0, $w, $h)
    $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $stride = $data.Stride
    $xor = [byte[]]::new($w * $h * 4)
    for ($row = 0; $row -lt $h; $row++) {
        $srcY = $h - 1 - $row
        $src = [IntPtr]($data.Scan0.ToInt64() + $srcY * $stride)
        [System.Runtime.InteropServices.Marshal]::Copy($src, $xor, $row * $w * 4, $w * 4)
    }
    $bmp.UnlockBits($data)
    $maskRow = [int]([Math]::Floor(($w + 31) / 32) * 4)
    $mask = [byte[]]::new($maskRow * $h)

    $ms = [System.IO.MemoryStream]::new()
    $bw = [System.IO.BinaryWriter]::new($ms)
    $bw.Write([uint32]40)
    $bw.Write([int32]$w)
    $bw.Write([int32]($h * 2))
    $bw.Write([uint16]1)
    $bw.Write([uint16]32)
    $bw.Write([uint32]0)
    $bw.Write([uint32]($xor.Length + $mask.Length))
    $bw.Write([int32]0)
    $bw.Write([int32]0)
    $bw.Write([uint32]0)
    $bw.Write([uint32]0)
    $bw.Write([byte[]]$xor)
    $bw.Write([byte[]]$mask)
    $bw.Flush()
    $bytes = $ms.ToArray()
    $bw.Dispose()
    $ms.Dispose()
    return $bytes
}

$sizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)
$frames = @()
foreach ($s in $sizes) {
    $bmp = New-IconBitmap $s
    if ($s -ge 256) {
        $ms = [System.IO.MemoryStream]::new()
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $bytes = $ms.ToArray()
        $ms.Dispose()
    } else {
        $bytes = Get-DibBytes $bmp
    }
    $frames += [pscustomobject]@{ Size = $s; Bytes = $bytes }
    if ($s -eq 256) { $bmp.Save($outPng, [System.Drawing.Imaging.ImageFormat]::Png) }
    $bmp.Dispose()
}

$count = $frames.Count
$ms = [System.IO.MemoryStream]::new()
$bw = [System.IO.BinaryWriter]::new($ms)
$bw.Write([uint16]0)
$bw.Write([uint16]1)
$bw.Write([uint16]$count)
$offset = 6 + 16 * $count
foreach ($f in $frames) {
    $dim = if ($f.Size -ge 256) { 0 } else { $f.Size }
    $bw.Write([byte]$dim)
    $bw.Write([byte]$dim)
    $bw.Write([byte]0)
    $bw.Write([byte]0)
    $bw.Write([uint16]1)
    $bw.Write([uint16]32)
    $bw.Write([uint32]$f.Bytes.Length)
    $bw.Write([uint32]$offset)
    $offset += $f.Bytes.Length
}
foreach ($f in $frames) { $bw.Write([byte[]]$f.Bytes) }
$bw.Flush()
[System.IO.File]::WriteAllBytes($outIco, $ms.ToArray())
$bw.Dispose()
$ms.Dispose()

# preview sheet: 16/24/32/48/256 enlarged
$sheet = [System.Drawing.Bitmap]::new(640, 160, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($sheet)
$g.Clear([System.Drawing.Color]::FromArgb(255, 32, 34, 38))
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
$x = 8
foreach ($s in @(16, 24, 32, 48, 64, 128)) {
    $b = New-IconBitmap $s
    $scale = [Math]::Floor(128 / $s)
    if ($scale -lt 1) { $scale = 1 }
    $g.DrawImage($b, $x, 8, $s * $scale, $s * $scale)
    $x += $s * $scale + 8
    $b.Dispose()
}
$g.Dispose()
$sheet.Save($preview, [System.Drawing.Imaging.ImageFormat]::Png)
$sheet.Dispose()

Write-Output ("wrote " + $outIco + " " + (Get-Item $outIco).Length + " bytes")
Write-Output ("wrote " + $outPng + " " + (Get-Item $outPng).Length + " bytes")
