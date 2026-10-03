[CmdletBinding(PositionalBinding = $false)]
param(
    [ValidateSet('doctor', 'cargo', 'rustc', 'rustup', 'npm')]
    [string]$Command = 'doctor',
    [ValidateSet('msvc', 'gnu')]
    [string]$Abi = 'msvc',
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CommandArgs = @()
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$localTools = Join-Path $projectRoot '.tools'
$toolchainBin = Join-Path $localTools "rustup\toolchains\stable-x86_64-pc-windows-$Abi\bin"
$localRustup = Join-Path $localTools 'rustup.exe'
$msvcLoaded = $false

# Environment changes apply only to this PowerShell process and its children.
if (Test-Path -LiteralPath $toolchainBin) {
    $env:CARGO_HOME = Join-Path $localTools 'cargo'
    $env:RUSTUP_HOME = Join-Path $localTools 'rustup'
    $env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-$Abi"
    $localCargoBin = Join-Path $localTools 'cargo\bin'
    $env:PATH = "$toolchainBin;$localCargoBin;$env:PATH"
}

if ($Abi -eq 'msvc') {
    $vswhereCandidates = @(
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe",
        "$env:ProgramFiles\Microsoft Visual Studio\Installer\vswhere.exe"
    )
    foreach ($candidate in $vswhereCandidates) {
        if (Test-Path -LiteralPath $candidate) {
            $installationPath = & $candidate -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
            if ($installationPath) {
                $developerCommand = Join-Path $installationPath 'Common7\Tools\VsDevCmd.bat'
                if (Test-Path -LiteralPath $developerCommand) {
                    $environmentLines = & $env:ComSpec /d /s /c "`"`"$developerCommand`" -arch=x64 -host_arch=x64 >nul && set`""
                    foreach ($line in $environmentLines) {
                        if ($line -match '^([^=]+)=(.*)$') {
                            [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process')
                        }
                    }
                    $msvcLoaded = $LASTEXITCODE -eq 0
                }
            }
            break
        }
    }
} else {
    $gnuCandidates = @('C:\msys64\ucrt64\bin', 'C:\msys64\mingw64\bin')
    foreach ($candidate in $gnuCandidates) {
        if (Test-Path -LiteralPath (Join-Path $candidate 'gcc.exe')) {
            $env:PATH = "$candidate;$env:PATH"
            $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $candidate 'gcc.exe'
            break
        }
    }
}

if ($Command -eq 'doctor') {
    Write-Output "Project: $projectRoot"
    Write-Output "Requested Rust ABI: $Abi"
    $cargoCommand = Get-Command cargo.exe -ErrorAction SilentlyContinue
    $rustCommand = Get-Command rustc.exe -ErrorAction SilentlyContinue
    if ($cargoCommand) { & $cargoCommand.Source --version } else { Write-Output 'Cargo: not found' }
    if ($rustCommand) { & $rustCommand.Source --version } else { Write-Output 'Rust: not found' }
    if ($Abi -eq 'msvc') {
        if ($msvcLoaded -or (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
            Write-Output 'MSVC: available'
        } else {
            Write-Output 'MSVC: not found; desktop builds require C++ Build Tools and a Windows SDK.'
        }
    } else {
        $gccCommand = Get-Command gcc.exe -ErrorAction SilentlyContinue
        if ($gccCommand) { Write-Output "GNU linker: $($gccCommand.Source)" } else { Write-Output 'GNU linker: not found' }
    }
    $runtime = Get-ItemProperty -Path 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\*' -ErrorAction SilentlyContinue | Where-Object { $_.name -like '*WebView2*' }
    if ($runtime) { Write-Output "WebView2: $($runtime.pv)" } else { Write-Output 'WebView2: not found in machine registry; a per-user installation may still exist.' }
    exit 0
}

if ($Command -eq 'rustup' -and (Test-Path -LiteralPath $localRustup)) {
    $executable = $localRustup
} elseif ($Command -eq 'npm') {
    $executable = (Get-Command npm.cmd -ErrorAction Stop).Source
} else {
    $executable = (Get-Command "$Command.exe" -ErrorAction Stop).Source
}

Push-Location $projectRoot
try {
    & $executable @CommandArgs
    $commandExitCode = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $commandExitCode
