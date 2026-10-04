param(
  [string]$Makensis = "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe",
  [string]$TemplateDirectory = "$PSScriptRoot\..\src-tauri\target\release\nsis\x64"
)

$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$scope = [guid]::NewGuid().ToString('N')
$root = Join-Path $repo "artifacts\installer-branding-$scope"
$registryPrefix = "Software\AhaXSmoke\$scope"
$registryRoot = "HKCU:\$registryPrefix"
$legacy = Join-Path $root 'Legacy Install'
$default = Join-Path $root 'Default Install'
$custom = Join-Path $root 'Explicit Install'
$other = Join-Path $root 'Other App'
$shortcuts = Join-Path $root 'shortcuts'
$legacyKey = "$registryRoot\Legacy"
$uninstallKey = "$registryRoot\Uninstall"
$currentKey = "$registryRoot\Current"
$results = [System.Collections.Generic.List[object]]::new()
$shell = $null

function ConvertTo-NsisLiteral([string]$value) {
  return $value.Replace('$', '$$').Replace('"', '$\"')
}

function Set-RegistryString([string]$key, [string]$name, [string]$value) {
  if (-not (Test-Path -LiteralPath $key)) { New-Item -Path $key -Force | Out-Null }
  if ($name -eq '') { Set-Item -LiteralPath $key -Value $value }
  else { New-ItemProperty -LiteralPath $key -Name $name -Value $value -PropertyType String -Force | Out-Null }
}

function Reset-Legacy {
  foreach ($key in @($legacyKey, $uninstallKey, $currentKey)) {
    if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse -Force }
  }
  Set-RegistryString $legacyKey '' $legacy
  Set-RegistryString $uninstallKey 'MainBinaryName' 'vela.exe'
  Set-RegistryString $uninstallKey 'UninstallString' "`"$legacy\uninstall.exe`""
  Set-RegistryString $uninstallKey 'InstallLocation' "`"$legacy`""
  Set-RegistryString $uninstallKey 'DisplayName' 'Vela'
  Set-RegistryString $uninstallKey 'Publisher' 'vela'
  Set-RegistryString $uninstallKey 'DisplayIcon' "`"$legacy\vela.exe`""
  [IO.File]::WriteAllText((Join-Path $legacy 'vela.exe'), 'Synthetic executable fixture; never execute.')
  [IO.File]::WriteAllText((Join-Path $legacy 'uninstall.exe'), 'Synthetic uninstaller fixture; never execute.')
}

function Invoke-Harness([string]$mode, [string]$initial = '', [string]$explicit = '', [string]$second = '') {
  $ini = "[case]`r`nmode=$mode`r`ninitial=$initial`r`nsecond=$second`r`n"
  [IO.File]::WriteAllText((Join-Path $root 'case.ini'), $ini, [Text.Encoding]::Unicode)
  $arguments = '/S'
  if ($explicit) { $arguments += " /D=$explicit" }
  $process = Start-Process -FilePath (Join-Path $root 'harness.exe') -ArgumentList $arguments -PassThru -WindowStyle Hidden
  if (-not $process.WaitForExit(20000)) {
    $process.Kill()
    throw 'Isolated installer harness timed out.'
  }
  if ($process.ExitCode -ne 0) { throw "Isolated harness returned $($process.ExitCode)." }
  return [IO.File]::ReadAllText((Join-Path $root 'result.txt'), [Text.Encoding]::Unicode).TrimEnd("`r", "`n")
}

function Assert-Result([string]$name, [bool]$condition) {
  if (-not $condition) { throw "Failed: $name" }
  $results.Add([pscustomobject]@{ name = $name; passed = $true })
  Write-Output "PASS $name"
}

function New-TestShortcut([string]$name, [string]$target, [string]$arguments = '') {
  $link = $shell.CreateShortcut((Join-Path $shortcuts $name))
  $link.TargetPath = $target
  $link.Arguments = $arguments
  $link.WorkingDirectory = [IO.Path]::GetDirectoryName($target)
  $link.Save()
}

function Clear-TestShortcuts {
  foreach ($name in @('Vela.lnk', 'AhaX.lnk')) {
    $path = Join-Path $shortcuts $name
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
  }
}

try {
  if (-not (Test-Path -LiteralPath $Makensis -PathType Leaf)) { throw 'Cached NSIS compiler is missing.' }
  $utils = Join-Path $TemplateDirectory 'utils.nsh'
  $template = Join-Path $TemplateDirectory 'installer.nsi'
  if (-not (Test-Path -LiteralPath $utils -PathType Leaf)) { throw 'Generate the Tauri NSIS template before running this smoke test.' }
  foreach ($directory in @($root, $legacy, $default, $custom, $other, $shortcuts)) {
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
  }
  [IO.File]::WriteAllText((Join-Path $other 'other.exe'), 'Different application fixture; never execute.')
  $shell = New-Object -ComObject WScript.Shell
  $script = @'
Unicode true
RequestExecutionLevel user
Name "AhaX isolated migration smoke"
OutFile "@@ROOT@@\harness.exe"
InstallDir "@@DEFAULT@@"
!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh
!include WordFunc.nsh
!include "@@UTILS@@"
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "Win\RestartManager.nsh"
!define INSTALLMODE "currentUser"
!define ARCH "x64"
!define BUNDLEID "app.ahax.smoke.@@SCOPE@@"
!define AHAX_LEGACY_PRODUCT_KEY "@@REGISTRY@@\Legacy"
!define AHAX_LEGACY_UNINSTALL_KEY "@@REGISTRY@@\Uninstall"
!define AHAX_PRODUCT_KEY "@@REGISTRY@@\Current"
!define AHAX_DEFAULT_INSTALL "@@DEFAULT@@"
Var UpdateMode
Var NoShortcutMode
!include "@@HOOKS@@"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Function .onInit
  !insertmacro SetContext
  ReadINIStr $R8 "@@ROOT@@\case.ini" "case" "mode"
  ReadINIStr $R9 "@@ROOT@@\case.ini" "case" "initial"
  ${If} $R9 != ""
    StrCpy $INSTDIR $R9
  ${EndIf}
FunctionEnd
Function CompileHooksWithoutRunning
  !insertmacro NSIS_HOOK_PREINSTALL
  !insertmacro NSIS_HOOK_POSTINSTALL
FunctionEnd
Section
  ${If} $R8 == "detect"
    Call AhaXFindLegacyInstall
    StrCpy $R7 $AhaXLegacyInstall
  ${ElseIf} $R8 == "restore"
    Call AhaXRestoreLegacyInstall
    StrCpy $R7 $INSTDIR
  ${ElseIf} $R8 == "twice"
    Call AhaXRestoreLegacyInstall
    ReadINIStr $INSTDIR "@@ROOT@@\case.ini" "case" "second"
    Call AhaXRestoreLegacyInstall
    StrCpy $R7 $INSTDIR
  ${ElseIf} $R8 == "remove"
    !insertmacro AhaXRemoveLegacyRegistration
    StrCpy $R7 "removed-if-matching"
  ${ElseIf} $R8 == "preinstall"
    !insertmacro NSIS_HOOK_PREINSTALL
    StrCpy $R7 "$INSTDIR|$NoShortcutMode"
  ${ElseIf} $R8 == "shortcut"
    !insertmacro AhaXMigrateShortcut "@@ROOT@@\shortcuts\Vela.lnk" "@@ROOT@@\shortcuts\AhaX.lnk"
    StrCpy $R7 "shortcut-checked"
  ${Else}
    SetErrorLevel 9
    Quit
  ${EndIf}
  FileOpen $R6 "@@ROOT@@\result.txt" w
  FileWriteUTF16LE $R6 "$R7"
  FileClose $R6
SectionEnd
'@
  $replacements = @{
    '@@ROOT@@' = $root; '@@DEFAULT@@' = $default; '@@UTILS@@' = [IO.Path]::GetFullPath($utils)
    '@@SCOPE@@' = $scope; '@@REGISTRY@@' = $registryPrefix
    '@@HOOKS@@' = (Join-Path $repo 'src-tauri\installer-hooks.nsh')
  }
  foreach ($entry in $replacements.GetEnumerator()) { $script = $script.Replace($entry.Key, (ConvertTo-NsisLiteral $entry.Value)) }
  $scriptPath = Join-Path $root 'harness.nsi'
  [IO.File]::WriteAllText($scriptPath, $script, [Text.UTF8Encoding]::new($false))
  & $Makensis /V2 $scriptPath *> (Join-Path $root 'harness-compile.log')
  if ($LASTEXITCODE -ne 0) { throw "NSIS harness compilation failed; inspect $root\harness-compile.log" }
  Assert-Result 'MUI GUIINIT and hooks compile with actual Tauri utility macros' $true

  Reset-Legacy
  Assert-Result 'Consistent legacy registration resolves its existing path' ((Invoke-Harness 'detect') -eq $legacy)
  Assert-Result 'Default destination reuses a valid legacy installation' ((Invoke-Harness 'restore') -eq $legacy)
  Assert-Result 'Explicit non-default destination is preserved' ((Invoke-Harness 'restore' '' $custom) -eq $custom)
  Assert-Result 'Explicit default destination is preserved' ((Invoke-Harness 'restore' '' $default) -eq $default)
  Assert-Result 'A later directory-page choice is not overwritten by the preinstall hook' ((Invoke-Harness 'twice' '' '' $default) -eq $default)
  Assert-Result 'Migration disables upstream shortcut creation before renaming existing links' ((Invoke-Harness 'preinstall') -eq "$legacy|1")
  Assert-Result 'A separate installation retains normal upstream shortcut behavior' ((Invoke-Harness 'preinstall' '' $custom) -eq "$custom|" )
  Set-RegistryString $currentKey '' $custom
  Assert-Result 'An existing new-brand installation prevents legacy override' ((Invoke-Harness 'restore') -eq $default)

  foreach ($case in @(
    @{ name = 'Different binary registration'; field = 'MainBinaryName'; value = 'other.exe' },
    @{ name = 'Foreign uninstall command'; field = 'UninstallString'; value = "`"$other\uninstall.exe`"" },
    @{ name = 'Mismatched install location'; field = 'InstallLocation'; value = "`"$other`"" },
    @{ name = 'Different application name'; field = 'DisplayName'; value = 'OtherApp' },
    @{ name = 'Different publisher'; field = 'Publisher'; value = 'other' },
    @{ name = 'Foreign icon executable'; field = 'DisplayIcon'; value = "`"$other\other.exe`"" }
  )) {
    Reset-Legacy
    Set-RegistryString $uninstallKey $case.field $case.value
    Assert-Result "$($case.name) is rejected" ((Invoke-Harness 'detect') -eq '')
    Invoke-Harness 'remove' $legacy | Out-Null
    Assert-Result "$($case.name) remains registered" ((Test-Path -LiteralPath $legacyKey) -and (Test-Path -LiteralPath $uninstallKey))
  }
  Reset-Legacy
  Remove-Item -LiteralPath (Join-Path $legacy 'uninstall.exe') -Force
  Assert-Result 'Missing legacy uninstaller is rejected' ((Invoke-Harness 'detect') -eq '')
  Reset-Legacy
  Remove-Item -LiteralPath (Join-Path $legacy 'vela.exe') -Force
  Assert-Result 'Missing legacy executable is rejected' ((Invoke-Harness 'detect') -eq '')

  Reset-Legacy
  Invoke-Harness 'remove' $custom | Out-Null
  Assert-Result 'Installing elsewhere preserves the old registration' ((Test-Path -LiteralPath $legacyKey) -and (Test-Path -LiteralPath $uninstallKey))
  Set-RegistryString $currentKey '' $legacy
  Invoke-Harness 'remove' $legacy | Out-Null
  Assert-Result 'Only matching legacy product and uninstall keys are removed' ((-not (Test-Path -LiteralPath $legacyKey)) -and (-not (Test-Path -LiteralPath $uninstallKey)) -and (Test-Path -LiteralPath $currentKey))

  New-TestShortcut 'Vela.lnk' (Join-Path $legacy 'vela.exe') '--fixture-argument'
  Invoke-Harness 'shortcut' $legacy | Out-Null
  $link = $shell.CreateShortcut((Join-Path $shortcuts 'AhaX.lnk'))
  Assert-Result 'Matching shortcut migrates while preserving target and arguments' ((-not (Test-Path -LiteralPath (Join-Path $shortcuts 'Vela.lnk'))) -and ($link.TargetPath -eq (Join-Path $legacy 'vela.exe')) -and ($link.Arguments -eq '--fixture-argument'))

  Clear-TestShortcuts
  New-TestShortcut 'Vela.lnk' (Join-Path $other 'other.exe')
  Invoke-Harness 'shortcut' $legacy | Out-Null
  Assert-Result 'A same-name shortcut to another application remains untouched' ((Test-Path -LiteralPath (Join-Path $shortcuts 'Vela.lnk')) -and (-not (Test-Path -LiteralPath (Join-Path $shortcuts 'AhaX.lnk'))))

  Clear-TestShortcuts
  New-TestShortcut 'Vela.lnk' (Join-Path $legacy 'vela.exe')
  New-TestShortcut 'AhaX.lnk' (Join-Path $other 'other.exe')
  Invoke-Harness 'shortcut' $legacy | Out-Null
  $link = $shell.CreateShortcut((Join-Path $shortcuts 'AhaX.lnk'))
  Assert-Result 'Conflicting destination shortcut is preserved together with the old shortcut' ((Test-Path -LiteralPath (Join-Path $shortcuts 'Vela.lnk')) -and ($link.TargetPath -eq (Join-Path $other 'other.exe')))

  Clear-TestShortcuts
  New-TestShortcut 'Vela.lnk' (Join-Path $legacy 'vela.exe')
  New-TestShortcut 'AhaX.lnk' (Join-Path $legacy 'vela.exe') '--keep-existing'
  Invoke-Harness 'shortcut' $legacy | Out-Null
  $link = $shell.CreateShortcut((Join-Path $shortcuts 'AhaX.lnk'))
  Assert-Result 'Matching existing destination is retained without losing its arguments' ((-not (Test-Path -LiteralPath (Join-Path $shortcuts 'Vela.lnk'))) -and ($link.Arguments -eq '--keep-existing'))

  if (Test-Path -LiteralPath $template -PathType Leaf) {
    $templateText = [IO.File]::ReadAllText($template)
    $hookPosition = $templateText.IndexOf('installer-hooks.nsh')
    $pagePosition = $templateText.IndexOf('!insertmacro MUI_PAGE_')
    Assert-Result 'Tauri imports migration hooks before expanding MUI pages' ($hookPosition -ge 0 -and $hookPosition -lt $pagePosition)
    $templateText = $templateText.Replace('!include "utils.nsh"', '!include "' + (ConvertTo-NsisLiteral ([IO.Path]::GetFullPath($utils))) + '"')
    $templateText = $templateText.Replace('!include "FileAssociation.nsh"', '!include "' + (ConvertTo-NsisLiteral ([IO.Path]::GetFullPath((Join-Path $TemplateDirectory 'FileAssociation.nsh')))) + '"')
    $templateText = [regex]::Replace($templateText, '(?m)^!define PRODUCTNAME ".*"', '!define PRODUCTNAME "AhaX"')
    $templateText = [regex]::Replace($templateText, '(?m)^!define OUTFILE ".*"', '!define OUTFILE "' + (ConvertTo-NsisLiteral (Join-Path $root 'compile-only-production-template.exe')) + '"')
    $compileOnly = Join-Path $root 'compile-only-production-template.nsi'
    [IO.File]::WriteAllText($compileOnly, $templateText, [Text.UTF8Encoding]::new($false))
    & $Makensis /V2 $compileOnly *> (Join-Path $root 'production-template-compile.log')
    if ($LASTEXITCODE -ne 0) { throw "Production-template compilation failed; inspect $root\production-template-compile.log" }
    Assert-Result 'Complete generated Tauri template compiles with the migration hooks' $true
  }
  $report = [pscustomobject]@{ passed = $results.Count; registryScope = $registryPrefix; cases = $results; artifacts = $root }
  $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $root 'results.json') -Encoding utf8
  Write-Output "Completed $($results.Count) isolated checks. Artifacts: $root"
}
finally {
  if ($registryRoot -notmatch '^HKCU:\\Software\\AhaXSmoke\\[a-f0-9]{32}$') { throw 'Refusing to clean an unexpected registry scope.' }
  if (Test-Path -LiteralPath $registryRoot) { Remove-Item -LiteralPath $registryRoot -Recurse -Force }
  if ($shell) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell) }
}
