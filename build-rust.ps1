[CmdletBinding()]
param(
    [ValidateSet('Debug','Release')][string]$Configuration='Release',
    [string]$ToolchainDirectory,
    [switch]$Test,
    [string]$RequiredHost,
    [string[]]$CargoArguments=@()
)
$ErrorActionPreference='Stop'
$repo=$PSScriptRoot
$initialLocation=(Get-Location).Path
$resolvedToolchain=if($ToolchainDirectory){[IO.Path]::GetFullPath($ToolchainDirectory)}else{$null}
$saved=@{}
foreach($name in @('RUSTUP_HOME','CARGO_HOME','PATH','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS')){$saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
try {
    Set-Location -LiteralPath $repo
    $flags=@()
    $localHome=[Environment]::GetFolderPath('UserProfile')
    if($localHome){$flags+="--remap-path-prefix=$localHome=/home"}
    if($ToolchainDirectory){
        $tools=$resolvedToolchain
        $env:RUSTUP_HOME=Join-Path $tools 'rustup'
        $env:CARGO_HOME=Join-Path $tools 'cargo'
        $env:PATH=(Join-Path $env:CARGO_HOME 'bin')+';'+$env:PATH
        $mingw=Get-ChildItem -LiteralPath (Join-Path $tools 'llvm-mingw') -Directory -ErrorAction SilentlyContinue | Select-Object -First 1
        if($mingw){$env:PATH=(Join-Path $mingw.FullName 'bin')+';'+$env:PATH}
        $gnu=Join-Path $tools 'gnu/w64devkit/bin'
        if(Test-Path -LiteralPath (Join-Path $gnu 'gcc.exe')){$env:PATH=$gnu+';'+$env:PATH}
        $flags+="--remap-path-prefix=$tools=/toolchain"
    }
    $cargo=(Get-Command cargo -ErrorAction Stop).Source
    $compilerInfo=& rustc -vV
    if($LASTEXITCODE -ne 0){throw 'Rust compiler could not run'}
    if ($RequiredHost -and $compilerInfo -notcontains "host: $RequiredHost") {
        throw "This package requires the $RequiredHost Rust toolchain."
    }
    if($compilerInfo -match 'host:.*windows-gnu'){$flags+=@('-C','link-self-contained=yes')}
    foreach($name in @('CARGO_HOME','RUSTUP_HOME')){if([Environment]::GetEnvironmentVariable($name)){$flags+="--remap-path-prefix=$([Environment]::GetEnvironmentVariable($name))=/$($name.ToLowerInvariant())"}}
    # The last matching remap wins. Put the most specific source root last.
    $flags+="--remap-path-prefix=$repo=/src"
    if($env:CARGO_TARGET_DIR){
        $targetDirectory=[IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
        if(-not $targetDirectory.StartsWith("$repo\",[StringComparison]::OrdinalIgnoreCase)){
            $flags+="--remap-path-prefix=$targetDirectory=/target"
        }
    }
    $env:RUSTFLAGS=$null
    $env:CARGO_ENCODED_RUSTFLAGS=$flags -join [char]31
    if($CargoArguments.Count){
        $arguments=@($CargoArguments[0],'--manifest-path',(Join-Path $repo 'Cargo.toml'))
        if($CargoArguments.Count -gt 1){$arguments+=$CargoArguments[1..($CargoArguments.Count-1)]}
        & $cargo @arguments
    }
    elseif($Test){& $cargo test --locked --workspace --manifest-path (Join-Path $repo 'Cargo.toml')}
    else{
        $arguments=@('build','--locked','--workspace','--manifest-path',(Join-Path $repo 'Cargo.toml'))
        if($Configuration -eq 'Release'){$arguments+='--release'}
        & $cargo @arguments
    }
    if($LASTEXITCODE -ne 0){throw 'Rust command failed'}
} finally {foreach($name in $saved.Keys){[Environment]::SetEnvironmentVariable($name,$saved[$name],'Process')}; Set-Location -LiteralPath $initialLocation}
