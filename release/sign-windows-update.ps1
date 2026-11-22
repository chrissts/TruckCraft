[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$')][string]$Tag,
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$SigningKey,
    [string]$OpenSsl = 'openssl'
)

$ErrorActionPreference = 'Stop'
$file = Get-Item -LiteralPath $Executable
$keyPath = (Resolve-Path -LiteralPath $SigningKey).ProviderPath
if ($file.Name -cne 'TruckCraft.exe' -or $file.Length -lt 1 -or $file.Length -gt 100MB) {
    throw 'Pass the TruckCraft.exe asset from the draft release.'
}
$version = $file.VersionInfo
if ("v$($version.FileMajorPart).$($version.FileMinorPart).$($version.FileBuildPart)" -cne $Tag) {
    throw 'The executable version does not match the release tag.'
}
$digest = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
$payload = New-TemporaryFile
$signature = New-TemporaryFile
$publicKey = New-TemporaryFile
try {
    & $OpenSsl pkey -in $keyPath -pubout -outform DER -out $publicKey.FullName
    if ($LASTEXITCODE -ne 0) { throw 'Could not read the public signing key.' }
    $expected = '302a300506032b6570032100' + (Get-Content -Raw -LiteralPath "$PSScriptRoot/windows-update-public-key.txt").Trim()
    $actual = [Convert]::ToHexString([IO.File]::ReadAllBytes($publicKey.FullName)).ToLowerInvariant()
    if ($actual -cne $expected) { throw 'The private key does not match the public key shipped with TruckCraft.' }

    $size = $file.Length.ToString([Globalization.CultureInfo]::InvariantCulture)
    [IO.File]::WriteAllText($payload.FullName, "TruckCraft Windows x64 update v1`n$Tag`n$size`n$digest`n", [Text.Encoding]::ASCII)
    & $OpenSsl pkeyutl -sign -rawin -inkey $keyPath -in $payload.FullName -out $signature.FullName
    if ($LASTEXITCODE -ne 0) { throw 'Could not sign the update metadata.' }
    $bytes = [IO.File]::ReadAllBytes($signature.FullName)
    if ($bytes.Length -ne 64) { throw 'Expected a 64-byte Ed25519 signature.' }
    $hex = [Convert]::ToHexString($bytes).ToLowerInvariant()
    "<!-- truckcraft-windows-update-v1 $Tag $size $digest $hex -->"
}
finally {
    Remove-Item -LiteralPath $payload.FullName, $signature.FullName, $publicKey.FullName
}
