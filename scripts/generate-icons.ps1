param(
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '..\assets\icons')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$sourceDirectory = Join-Path $PSScriptRoot '..\assets\icons\source'
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null

function Get-ArtworkBounds([Drawing.Bitmap]$Bitmap) {
    $left = $Bitmap.Width
    $top = $Bitmap.Height
    $right = -1
    $bottom = -1
    for ($y = 0; $y -lt $Bitmap.Height; $y++) {
        for ($x = 0; $x -lt $Bitmap.Width; $x++) {
            if ($Bitmap.GetPixel($x, $y).A -gt 0) {
                $left = [Math]::Min($left, $x)
                $top = [Math]::Min($top, $y)
                $right = [Math]::Max($right, $x)
                $bottom = [Math]::Max($bottom, $y)
            }
        }
    }
    if ($right -lt $left) {
        throw 'Source icon has no visible artwork.'
    }
    return [Drawing.Rectangle]::new($left, $top, $right - $left + 1, $bottom - $top + 1)
}

function Convert-IconPng {
    param([Drawing.Bitmap]$Source, [Drawing.Rectangle]$Bounds, [int]$Size, [double]$Inset)
    $bitmap = [Drawing.Bitmap]::new($Size, $Size, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $attributes = [Drawing.Imaging.ImageAttributes]::new()
    try {
        $graphics.CompositingMode = [Drawing.Drawing2D.CompositingMode]::SourceCopy
        $graphics.CompositingQuality = [Drawing.Drawing2D.CompositingQuality]::HighQuality
        $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        # Clamp sampling to the source edge: an opaque macOS canvas must stay opaque.
        $attributes.SetWrapMode([Drawing.Drawing2D.WrapMode]::TileFlipXY)
        $graphics.Clear([Drawing.Color]::Transparent)
        $scale = $Size * (1 - 2 * $Inset) / [Math]::Max($Bounds.Width, $Bounds.Height)
        $width = [int][Math]::Round($Bounds.Width * $scale)
        $height = [int][Math]::Round($Bounds.Height * $scale)
        $destination = [Drawing.Rectangle]::new(
            [int][Math]::Floor(($Size - $width) / 2),
            [int][Math]::Floor(($Size - $height) / 2), $width, $height
        )
        $graphics.DrawImage($Source, $destination, $Bounds.X, $Bounds.Y, $Bounds.Width,
            $Bounds.Height, [Drawing.GraphicsUnit]::Pixel, $attributes)
        $stream = [IO.MemoryStream]::new()
        try {
            $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
            return ,$stream.ToArray()
        } finally {
            $stream.Dispose()
        }
    } finally {
        $attributes.Dispose()
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

function Write-BigEndian([IO.BinaryWriter]$Writer, [uint32]$Value) {
    $Writer.Write([byte](($Value -shr 24) -band 255))
    $Writer.Write([byte](($Value -shr 16) -band 255))
    $Writer.Write([byte](($Value -shr 8) -band 255))
    $Writer.Write([byte]($Value -band 255))
}

function Convert-PngToArgb([byte[]]$Png) {
    $inputStream = [IO.MemoryStream]::new($Png, $false)
    $bitmap = [Drawing.Bitmap]::FromStream($inputStream)
    $outputStream = [IO.MemoryStream]::new()
    $writer = [IO.BinaryWriter]::new($outputStream)
    try {
        $writer.Write([Text.Encoding]::ASCII.GetBytes('ARGB'))
        # ICNS 16/32px entries use separate straight-alpha A/R/G/B planes. RLE
        # literal packets (up to 128 bytes) avoid PNG-in-legacy-slot corruption.
        foreach ($channel in @('A', 'R', 'G', 'B')) {
            $plane = [byte[]]::new($bitmap.Width * $bitmap.Height)
            for ($y = 0; $y -lt $bitmap.Height; $y++) {
                for ($x = 0; $x -lt $bitmap.Width; $x++) {
                    $plane[$y * $bitmap.Width + $x] = $bitmap.GetPixel($x, $y).$channel
                }
            }
            for ($offset = 0; $offset -lt $plane.Length; $offset += 128) {
                $count = [Math]::Min(128, $plane.Length - $offset)
                $writer.Write([byte]($count - 1))
                $writer.Write($plane, $offset, $count)
            }
        }
        return ,$outputStream.ToArray()
    } finally {
        $writer.Dispose()
        $outputStream.Dispose()
        $bitmap.Dispose()
        $inputStream.Dispose()
    }
}

$windowsSource = [Drawing.Bitmap]::FromFile((Join-Path $sourceDirectory 'tiny-shell-brand.png'))
$macosSource = [Drawing.Bitmap]::FromFile((Join-Path $sourceDirectory 'tiny-shell-macos.png'))
try {
    $artworkBounds = Get-ArtworkBounds $windowsSource
    $windowsFrames = @{}
    foreach ($size in @(16, 20, 24, 32, 40, 48, 64, 128, 256, 1024)) {
        $windowsFrames[$size] = Convert-IconPng $windowsSource $artworkBounds $size 0.02
    }
    [IO.File]::WriteAllBytes((Join-Path $outputRoot 'tiny-shell.png'), $windowsFrames[1024])
    $linuxDirectory = Join-Path $outputRoot '256x256'
    New-Item -ItemType Directory -Force -Path $linuxDirectory | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $linuxDirectory 'tiny-shell.png'), $windowsFrames[256])

    $icoSizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)
    $icoStream = [IO.MemoryStream]::new()
    $icoWriter = [IO.BinaryWriter]::new($icoStream)
    try {
        $icoWriter.Write([uint16]0)
        $icoWriter.Write([uint16]1)
        $icoWriter.Write([uint16]$icoSizes.Count)
        $offset = 6 + 16 * $icoSizes.Count
        foreach ($size in $icoSizes) {
            $dimension = if ($size -eq 256) { 0 } else { $size }
            $payload = $windowsFrames[$size]
            $icoWriter.Write([byte]$dimension)
            $icoWriter.Write([byte]$dimension)
            $icoWriter.Write([byte]0)
            $icoWriter.Write([byte]0)
            $icoWriter.Write([uint16]1)
            $icoWriter.Write([uint16]32)
            $icoWriter.Write([uint32]$payload.Length)
            $icoWriter.Write([uint32]$offset)
            $offset += $payload.Length
        }
        foreach ($size in $icoSizes) {
            $icoWriter.Write([byte[]]$windowsFrames[$size])
        }
        [IO.File]::WriteAllBytes((Join-Path $outputRoot 'tiny-shell.ico'), $icoStream.ToArray())
    } finally {
        $icoWriter.Dispose()
        $icoStream.Dispose()
    }

    $macosBounds = [Drawing.Rectangle]::new(0, 0, $macosSource.Width, $macosSource.Height)
    if ($macosSource.Width -ne $macosSource.Height) {
        throw 'macOS source must have a square full-bleed canvas.'
    }
    $macosFrames = @{}
    foreach ($size in @(16, 32, 64, 128, 256, 512, 1024)) {
        $macosFrames[$size] = Convert-IconPng $macosSource $macosBounds $size 0
    }
    $entries = [ordered]@{
        ic04 = (Convert-PngToArgb $macosFrames[16])
        ic11 = $macosFrames[32]
        ic05 = (Convert-PngToArgb $macosFrames[32])
        ic12 = $macosFrames[64]
        ic07 = $macosFrames[128]
        ic13 = $macosFrames[256]
        ic08 = $macosFrames[256]
        ic14 = $macosFrames[512]
        ic09 = $macosFrames[512]
        ic10 = $macosFrames[1024]
    }
    $length = 8
    foreach ($payload in $entries.Values) { $length += 8 + $payload.Length }
    $icnsStream = [IO.MemoryStream]::new()
    $icnsWriter = [IO.BinaryWriter]::new($icnsStream)
    try {
        $icnsWriter.Write([Text.Encoding]::ASCII.GetBytes('icns'))
        Write-BigEndian $icnsWriter $length
        foreach ($entry in $entries.GetEnumerator()) {
            $icnsWriter.Write([Text.Encoding]::ASCII.GetBytes($entry.Key))
            Write-BigEndian $icnsWriter (8 + $entry.Value.Length)
            $icnsWriter.Write([byte[]]$entry.Value)
        }
        [IO.File]::WriteAllBytes((Join-Path $outputRoot 'tiny-shell.icns'), $icnsStream.ToArray())
    } finally {
        $icnsWriter.Dispose()
        $icnsStream.Dispose()
    }
    Write-Output "Generated Windows, Linux and macOS icons in $outputRoot"
} finally {
    $windowsSource.Dispose()
    $macosSource.Dispose()
}
