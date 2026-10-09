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
    param(
        [Drawing.Bitmap]$Source, [Drawing.Rectangle]$Bounds, [int]$Size, [double]$Inset,
        [Drawing.Color]$Background = [Drawing.Color]::Transparent
    )
    $bitmap = [Drawing.Bitmap]::new($Size, $Size, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $attributes = [Drawing.Imaging.ImageAttributes]::new()
    try {
        $graphics.CompositingMode = if ($Background.A -eq 255) {
            [Drawing.Drawing2D.CompositingMode]::SourceOver
        } else {
            [Drawing.Drawing2D.CompositingMode]::SourceCopy
        }
        $graphics.CompositingQuality = [Drawing.Drawing2D.CompositingQuality]::HighQuality
        $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        # Clamp sampling to the source edge: an opaque macOS canvas must stay opaque.
        $attributes.SetWrapMode([Drawing.Drawing2D.WrapMode]::TileFlipXY)
        # Export in a neutral grayscale color mode; the approved AI master has
        # negligible RGB noise in its white edges, not intentional colored accents.
        $grayscale = [Drawing.Imaging.ColorMatrix]::new([single[][]]@(
            @(0.299, 0.299, 0.299, 0, 0),
            @(0.587, 0.587, 0.587, 0, 0),
            @(0.114, 0.114, 0.114, 0, 0),
            @(0, 0, 0, 1, 0),
            @(0, 0, 0, 0, 1)
        ))
        $attributes.SetColorMatrix($grayscale)
        $graphics.Clear($Background)
        $scale = $Size * (1 - 2 * $Inset) / [Math]::Max($Bounds.Width, $Bounds.Height)
        $width = [int][Math]::Round($Bounds.Width * $scale)
        $height = [int][Math]::Round($Bounds.Height * $scale)
        $destination = [Drawing.Rectangle]::new(
            [int][Math]::Floor(($Size - $width) / 2),
            [int][Math]::Floor(($Size - $height) / 2), $width, $height
        )
        $graphics.DrawImage($Source, $destination, $Bounds.X, $Bounds.Y, $Bounds.Width,
            $Bounds.Height, [Drawing.GraphicsUnit]::Pixel, $attributes)
        if ($Size -eq 16) {
            # At 16px, the terminal outline is subpixel-width and bicubic sampling
            # averages its brightest pixel down to gray. Restore optical contrast
            # after sampling, keeping geometry and alpha untouched.
            for ($y = 0; $y -lt $Size; $y++) {
                for ($x = 0; $x -lt $Size; $x++) {
                    $pixel = $bitmap.GetPixel($x, $y)
                    $gray = [int][Math]::Min(255, [Math]::Round($pixel.R * 1.2))
                    $bitmap.SetPixel($x, $y, [Drawing.Color]::FromArgb($pixel.A, $gray, $gray, $gray))
                }
            }
        }
        if ($Background.A -eq 0) {
            # A fractional 2% inset at 16px can leave a faint sampling fringe in
            # a corner. Keep the four outermost pixels fully transparent.
            $bitmap.SetPixel(0, 0, [Drawing.Color]::Transparent)
            $bitmap.SetPixel($Size - 1, 0, [Drawing.Color]::Transparent)
            $bitmap.SetPixel(0, $Size - 1, [Drawing.Color]::Transparent)
            $bitmap.SetPixel($Size - 1, $Size - 1, [Drawing.Color]::Transparent)
        }
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
        throw 'macOS source must have a square canvas.'
    }
    $macosFrames = @{}
    foreach ($size in @(16, 32, 64, 128, 256, 512, 1024)) {
        # The OS owns the outer mask/material: composite the approved transparent
        # master onto full-bleed black rather than baking a second rounded tile.
        $macosFrames[$size] = Convert-IconPng $macosSource $macosBounds $size 0 ([Drawing.Color]::Black)
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
