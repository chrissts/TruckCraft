[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$ResvgPath)
$ErrorActionPreference='Stop'
$truckcraftSource=Join-Path $PSScriptRoot 'truckcraft-icon.svg'
& $ResvgPath --width 1024 $truckcraftSource (Join-Path $PSScriptRoot 'truckcraft-icon.png')
if($LASTEXITCODE -ne 0){throw 'Brand PNG rendering failed'}
$truckcraftAppIcon=Join-Path $PSScriptRoot '../../crates/desktop/assets/app-mark-v2.png'
& $ResvgPath --width 512 $truckcraftSource $truckcraftAppIcon
if($LASTEXITCODE -ne 0){throw 'Application icon rendering failed'}

$sizes = 16, 24, 32, 48, 64, 128, 256
$pngPath = [IO.Path]::GetTempFileName()
$frames = [Collections.Generic.List[byte[]]]::new()
try {
    foreach ($size in $sizes) {
        & $ResvgPath --width $size $truckcraftSource $pngPath
        if ($LASTEXITCODE -ne 0) { throw "Icon frame rendering failed at ${size}px" }
        $frames.Add([IO.File]::ReadAllBytes($pngPath))
    }
} finally {
    Remove-Item -LiteralPath $pngPath
}

$icoPath = Join-Path $PSScriptRoot '../../crates/desktop/assets/truckcraft.ico'
$writer = [IO.BinaryWriter]::new([IO.File]::Create($icoPath))
try {
    $writer.Write([uint16]0)
    $writer.Write([uint16]1)
    $writer.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $dimension = [byte]($sizes[$i] % 256)
        $writer.Write($dimension)
        $writer.Write($dimension)
        $writer.Write([uint16]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]32)
        $writer.Write([uint32]$frames[$i].Length)
        $writer.Write([uint32]$offset)
        $offset += $frames[$i].Length
    }
    foreach ($frame in $frames) { $writer.Write($frame) }
} finally {
    $writer.Dispose()
}
