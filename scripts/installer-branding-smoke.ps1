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
$desktopDirectory = Join-Path $root 'desktop'
$startMenuDirectory = Join-Path $root 'start-menu'
$legacyKey = "$registryRoot\Legacy"
$uninstallKey = "$registryRoot\Uninstall"
$currentKey = "$registryRoot\Current"
$previousKey = "$registryRoot\Previous"
$previousUninstallKey = "$registryRoot\PreviousUninstall"
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
  foreach ($key in @($legacyKey, $uninstallKey, $currentKey, $previousKey, $previousUninstallKey)) {
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

function Reset-Previous {
  Reset-Legacy
  Set-RegistryString $previousKey '' $legacy
  foreach ($field in @('MainBinaryName', 'UninstallString', 'InstallLocation', 'Publisher', 'DisplayIcon')) {
    Set-RegistryString $previousUninstallKey $field (Get-ItemPropertyValue -LiteralPath $uninstallKey -Name $field)
  }
  Set-RegistryString $previousUninstallKey 'DisplayName' 'AhaX'
  Remove-Item -LiteralPath $legacyKey -Recurse -Force
  Remove-Item -LiteralPath $uninstallKey -Recurse -Force
}

function Invoke-Harness([string]$mode, [string]$initial = '', [string]$explicit = '', [string]$second = '', [int]$expectedExit = 0) {
  $ini = "[case]`r`nmode=$mode`r`ninitial=$initial`r`nsecond=$second`r`n"
  [IO.File]::WriteAllText((Join-Path $root 'case.ini'), $ini, [Text.Encoding]::Unicode)
  $arguments = '/S'
  if ($explicit) { $arguments += " /D=$explicit" }
  $process = Start-Process -FilePath (Join-Path $root 'harness.exe') -ArgumentList $arguments -PassThru -WindowStyle Hidden
  if (-not $process.WaitForExit(20000)) {
    $process.Kill()
    throw 'Isolated installer harness timed out.'
  }
  if ($process.ExitCode -ne $expectedExit) { throw "Isolated harness returned $($process.ExitCode), expected $expectedExit." }
  if ($expectedExit -ne 0) { return 'aborted-safely' }
  return [IO.File]::ReadAllText((Join-Path $root 'result.txt'), [Text.Encoding]::Unicode).TrimEnd("`r", "`n")
}

function Assert-Result([string]$name, [bool]$condition) {
  if (-not $condition) { throw "Failed: $name" }
  $results.Add([pscustomobject]@{ name = $name; passed = $true })
  Write-Output "PASS $name"
}

function New-TestShortcut([string]$name, [string]$target, [string]$arguments = '', [string]$directory = $shortcuts) {
  $link = $shell.CreateShortcut((Join-Path $directory $name))
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
  foreach ($directory in @($root, $legacy, $default, $custom, $other, $shortcuts, $desktopDirectory, $startMenuDirectory)) {
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
!addplugindir "@@PLUGINS@@"
!define INSTALLMODE "currentUser"
!define ARCH "x64"
!define BUNDLEID "app.ahax.smoke.@@SCOPE@@"
!define AHAX_LEGACY_PRODUCT_KEY "@@REGISTRY@@\Legacy"
!define AHAX_LEGACY_UNINSTALL_KEY "@@REGISTRY@@\Uninstall"
!define AHAX_PREVIOUS_PRODUCT_KEY "@@REGISTRY@@\Previous"
!define AHAX_PREVIOUS_UNINSTALL_KEY "@@REGISTRY@@\PreviousUninstall"
!define AHAX_CURRENT_UNINSTALL_KEY "@@REGISTRY@@\PreviousUninstall"
!define AHAX_PRODUCT_KEY "@@REGISTRY@@\Current"
!define AHAX_DEFAULT_INSTALL "@@DEFAULT@@"
!define AHAX_DESKTOP_DIRECTORY "@@ROOT@@\desktop"
!define AHAX_STARTMENU_DIRECTORY "@@ROOT@@\start-menu"
Var UpdateMode
Var NoShortcutMode
Var PassiveMode
Var OldMainBinaryName
!include "@@HOOKS@@"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
LangString appRunning 1033 "Application is running"
LangString appRunningOkKill 1033 "Close application"
LangString failedToKillApp 1033 "Unable to close application"
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
  ${ElseIf} $R8 == "previous-shortcut"
    !insertmacro AhaXMigrateShortcut "@@ROOT@@\shortcuts\AhaX.lnk" "@@ROOT@@\shortcuts\ahaX.lnk"
    StrCpy $R7 "previous-shortcut-checked"
  ${ElseIf} $R8 == "install"
  ${OrIf} $R8 == "missing-new-binary"
  ${OrIf} $R8 == "failed-install"
    !insertmacro NSIS_HOOK_PREINSTALL
    ${If} $R8 != "missing-new-binary"
      FileOpen $R6 "$INSTDIR\ahax.exe" w
      FileWrite $R6 "Synthetic new binary fixture; never execute."
      FileClose $R6
    ${EndIf}
    ReadRegStr $OldMainBinaryName SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "MainBinaryName"
    ${If} $OldMainBinaryName != ""
    ${AndIf} $OldMainBinaryName != "ahax.exe"
      Delete "$INSTDIR\$OldMainBinaryName"
    ${EndIf}
    ${If} $R8 == "failed-install"
      SetErrorLevel 24
      Abort "Synthetic failure after the standard binary cleanup stage."
    ${EndIf}
    WriteRegStr SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "MainBinaryName" "ahax.exe"
    WriteRegStr SHCTX "${AHAX_PRODUCT_KEY}" "" "$INSTDIR"
    WriteRegStr SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
    WriteRegStr SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "InstallLocation" '$\"$INSTDIR$\"'
    WriteRegStr SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "DisplayName" "ahaX"
    WriteRegStr SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "Publisher" "ahaX"
    WriteRegStr SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "DisplayIcon" '$\"$INSTDIR\ahax.exe$\"'
    !insertmacro NSIS_HOOK_POSTINSTALL
    StrCpy $R7 "install-completed"
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
    '@@PLUGINS@@' = (Join-Path (Split-Path -Parent $Makensis) 'Plugins\x86-unicode\additional')
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
  Assert-Result 'Matching shortcut migrates to the new executable and preserves arguments' ((-not (Test-Path -LiteralPath (Join-Path $shortcuts 'Vela.lnk'))) -and ($link.TargetPath -eq (Join-Path $legacy 'ahax.exe')) -and ($link.Arguments -eq '--fixture-argument'))
  Assert-Result 'Migrated shortcut uses the new executable icon' ($link.IconLocation -eq "$(Join-Path $legacy 'ahax.exe'),0")

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
  $oldLink = $shell.CreateShortcut((Join-Path $shortcuts 'Vela.lnk'))
  Assert-Result 'Conflicting name keeps a working old-named shortcut to the new executable' ($oldLink.TargetPath -eq (Join-Path $legacy 'ahax.exe'))

  Clear-TestShortcuts
  New-TestShortcut 'Vela.lnk' (Join-Path $legacy 'vela.exe')
  New-TestShortcut 'AhaX.lnk' (Join-Path $legacy 'vela.exe') '--keep-existing'
  Invoke-Harness 'shortcut' $legacy | Out-Null
  $link = $shell.CreateShortcut((Join-Path $shortcuts 'AhaX.lnk'))
  Assert-Result 'Matching existing destination is retained without losing its arguments' ((-not (Test-Path -LiteralPath (Join-Path $shortcuts 'Vela.lnk'))) -and ($link.Arguments -eq '--keep-existing'))

  Clear-TestShortcuts
  New-TestShortcut 'AhaX.lnk' (Join-Path $legacy 'vela.exe') '--previous-version'
  Invoke-Harness 'previous-shortcut' $legacy | Out-Null
  $link = $shell.CreateShortcut((Join-Path $shortcuts 'ahaX.lnk'))
  Assert-Result 'Existing mixed-case product shortcut updates in place without deleting itself' ((Test-Path -LiteralPath (Join-Path $shortcuts 'ahaX.lnk')) -and ($link.TargetPath -eq (Join-Path $legacy 'ahax.exe')) -and ($link.Arguments -eq '--previous-version'))
  Assert-Result 'Existing product shortcut icon follows the renamed binary' ($link.IconLocation -eq "$(Join-Path $legacy 'ahax.exe'),0")

  Reset-Previous
  Assert-Result 'The previous mixed-case brand resolves its registered installation' ((Invoke-Harness 'detect') -eq $legacy)
  Assert-Result 'The previous mixed-case brand retains its installation directory' ((Invoke-Harness 'restore') -eq $legacy)
  Assert-Result 'The previous brand preserves an explicit custom installation directory' ((Invoke-Harness 'restore' '' $custom) -eq $custom)
  Invoke-Harness 'remove' $legacy | Out-Null
  Assert-Result 'Previous publisher key is removed without deleting the reused uninstall registration' ((-not (Test-Path -LiteralPath $previousKey)) -and (Test-Path -LiteralPath $previousUninstallKey))
  Reset-Previous
  Set-RegistryString $previousUninstallKey 'Publisher' 'other'
  Assert-Result 'An unrelated previous-brand registration is rejected' ((Invoke-Harness 'detect') -eq '')
  Invoke-Harness 'remove' $legacy | Out-Null
  Assert-Result 'Rejected previous-brand registrations remain intact' ((Test-Path -LiteralPath $previousKey) -and (Test-Path -LiteralPath $previousUninstallKey))

  Reset-Previous
  $foreignBinary = Join-Path $custom 'vela.exe'
  [IO.File]::WriteAllText($foreignBinary, 'Unrelated destination data must remain unchanged.')
  Invoke-Harness 'install' '' $custom '' 21 | Out-Null
  Assert-Result 'An explicit directory with a foreign legacy-named file aborts before template deletion' ([IO.File]::ReadAllText($foreignBinary) -eq 'Unrelated destination data must remain unchanged.')
  Assert-Result 'Rejected destination keeps the registered old binary and registry intact' ((Test-Path -LiteralPath (Join-Path $legacy 'vela.exe')) -and ((Get-ItemPropertyValue -LiteralPath $previousUninstallKey -Name 'MainBinaryName') -eq 'vela.exe'))
  Assert-Result 'Rejected destination does not begin copying the new binary' (-not (Test-Path -LiteralPath (Join-Path $custom 'ahax.exe')))
  Remove-Item -LiteralPath $foreignBinary -Force
  Invoke-Harness 'install' '' $custom | Out-Null
  Assert-Result 'An explicit empty destination installs without deleting the old installation' ((Test-Path -LiteralPath (Join-Path $custom 'ahax.exe')) -and (Test-Path -LiteralPath (Join-Path $legacy 'vela.exe')) -and (Test-Path -LiteralPath $previousKey))

  foreach ($unsafeName in @('other.exe', '..\Other App\other.exe')) {
    Reset-Previous
    Set-RegistryString $previousUninstallKey 'MainBinaryName' $unsafeName
    Invoke-Harness 'install' '' $custom '' 21 | Out-Null
    Assert-Result "Unexpected template deletion target is rejected: $unsafeName" (([IO.File]::ReadAllText((Join-Path $other 'other.exe'))) -eq 'Different application fixture; never execute.')
  }

  Reset-Previous
  New-TestShortcut 'AhaX.lnk' (Join-Path $legacy 'vela.exe') '--full-upgrade' $desktopDirectory
  New-TestShortcut 'AhaX.lnk' (Join-Path $legacy 'vela.exe') '' $startMenuDirectory
  Invoke-Harness 'install' $legacy | Out-Null
  $link = $shell.CreateShortcut((Join-Path $desktopDirectory 'ahaX.lnk'))
  $menuLink = $shell.CreateShortcut((Join-Path $startMenuDirectory 'ahaX.lnk'))
  Assert-Result 'Full previous-brand sequence migrates desktop and start menu after template cleanup' (($link.TargetPath -eq (Join-Path $legacy 'ahax.exe')) -and ($menuLink.TargetPath -eq (Join-Path $legacy 'ahax.exe')) -and ($link.Arguments -eq '--full-upgrade'))
  Assert-Result 'Successful full upgrade removes only the old binary and publisher registration' ((-not (Test-Path -LiteralPath (Join-Path $legacy 'vela.exe'))) -and (Test-Path -LiteralPath (Join-Path $legacy 'ahax.exe')) -and (-not (Test-Path -LiteralPath $previousKey)) -and (Test-Path -LiteralPath $previousUninstallKey))

  Reset-Legacy
  New-TestShortcut 'Vela.lnk' (Join-Path $legacy 'vela.exe') '--name-conflict' $desktopDirectory
  New-TestShortcut 'ahaX.lnk' (Join-Path $other 'other.exe') '' $desktopDirectory
  Invoke-Harness 'install' $legacy | Out-Null
  $oldLink = $shell.CreateShortcut((Join-Path $desktopDirectory 'Vela.lnk'))
  $foreignLink = $shell.CreateShortcut((Join-Path $desktopDirectory 'ahaX.lnk'))
  Assert-Result 'Full legacy upgrade preserves a foreign shortcut and keeps the old-named entry usable' (($oldLink.TargetPath -eq (Join-Path $legacy 'ahax.exe')) -and ($oldLink.Arguments -eq '--name-conflict') -and ($foreignLink.TargetPath -eq (Join-Path $other 'other.exe')) -and (Test-Path -LiteralPath $oldLink.TargetPath))

  Reset-Previous
  $lockedShortcut = Join-Path $desktopDirectory 'ahaX.lnk'
  New-TestShortcut 'ahaX.lnk' (Join-Path $legacy 'vela.exe') '--read-only-shortcut' $desktopDirectory
  [IO.File]::SetAttributes($lockedShortcut, [IO.FileAttributes]::ReadOnly)
  try { Invoke-Harness 'install' $legacy | Out-Null }
  finally { [IO.File]::SetAttributes($lockedShortcut, [IO.FileAttributes]::Normal) }
  $link = $shell.CreateShortcut($lockedShortcut)
  Assert-Result 'A failed shortcut write restores the compatibility binary after template deletion' (($link.TargetPath -eq (Join-Path $legacy 'vela.exe')) -and (Test-Path -LiteralPath $link.TargetPath) -and ([IO.File]::ReadAllText($link.TargetPath) -eq 'Synthetic executable fixture; never execute.'))
  Assert-Result 'Shortcut failure still leaves the new application executable available' (Test-Path -LiteralPath (Join-Path $legacy 'ahax.exe'))
  Assert-Result 'Incomplete shortcut migration is recorded for a later installation' ((Get-ItemPropertyValue -LiteralPath $previousUninstallKey -Name 'ahaXLegacyMigrationPending') -eq '1')
  Invoke-Harness 'install' $legacy | Out-Null
  $link = $shell.CreateShortcut($lockedShortcut)
  Assert-Result 'A later installation retries the previously locked shortcut without losing arguments' (($link.TargetPath -eq (Join-Path $legacy 'ahax.exe')) -and ($link.Arguments -eq '--read-only-shortcut') -and (-not (Test-Path -LiteralPath (Join-Path $legacy 'vela.exe'))))
  Assert-Result 'Completed retry clears the pending migration marker' (-not (Get-Item -LiteralPath $previousUninstallKey).Property.Contains('ahaXLegacyMigrationPending'))

  Reset-Previous
  Remove-Item -LiteralPath (Join-Path $legacy 'ahax.exe') -Force
  Invoke-Harness 'missing-new-binary' $legacy '' '' 23 | Out-Null
  Assert-Result 'Missing new binary restores the previous program and aborts shortcut migration' ((Test-Path -LiteralPath (Join-Path $legacy 'vela.exe')) -and (Test-Path -LiteralPath $previousKey))
  Reset-Previous
  Invoke-Harness 'failed-install' $legacy '' '' 24 | Out-Null
  Assert-Result 'An installer failure after standard cleanup restores the original binary' ([IO.File]::ReadAllText((Join-Path $legacy 'vela.exe')) -eq 'Synthetic executable fixture; never execute.')

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
