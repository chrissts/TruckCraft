[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$DependencyTree,
    [Parameter(Mandatory)][string]$BinaryOutput,
    [string]$ToolchainDirectory
)
$ErrorActionPreference = 'Stop'
$wrapper = Join-Path (Split-Path $PSScriptRoot) 'build-rust.ps1'
$target = 'x86_64-pc-windows-gnu'
$env:TRUCKCRAFT_BUNDLE_LOCAL_ART = '0'
$binary = & $wrapper -RequiredHost $target -ToolchainDirectory $ToolchainDirectory -CargoArguments @('build','--locked','--release','-p','truckcraft','--message-format=json-render-diagnostics') |
    ForEach-Object { ConvertFrom-Json $_ } |
    Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'truckcraft' -and $_.executable } |
    ForEach-Object { $_.executable }
if (@($binary).Count -ne 1) { throw 'Expected one TruckCraft executable from Cargo.' }
Copy-Item -LiteralPath $binary -Destination $BinaryOutput
& $wrapper -ToolchainDirectory $ToolchainDirectory -CargoArguments @('tree','--locked','-p','truckcraft','-e','normal','--prefix','none','--target',$target) |
    Set-Content -Encoding utf8 -LiteralPath $DependencyTree
