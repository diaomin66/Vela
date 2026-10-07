!ifndef AHAX_LEGACY_PRODUCT_KEY
  !define AHAX_LEGACY_PRODUCT_KEY "Software\vela\Vela"
!endif
!ifndef AHAX_LEGACY_UNINSTALL_KEY
  !define AHAX_LEGACY_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vela"
!endif
!ifndef AHAX_PRODUCT_KEY
  !define AHAX_PRODUCT_KEY "Software\ahaX\ahaX"
!endif
!ifndef AHAX_PREVIOUS_PRODUCT_KEY
  !define AHAX_PREVIOUS_PRODUCT_KEY "Software\vela\AhaX"
!endif
!ifndef AHAX_PREVIOUS_UNINSTALL_KEY
  !define AHAX_PREVIOUS_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\AhaX"
!endif
!ifndef AHAX_DEFAULT_INSTALL
  !define AHAX_DEFAULT_INSTALL "$LOCALAPPDATA\ahaX"
!endif
!ifndef AHAX_CURRENT_UNINSTALL_KEY
  !define AHAX_CURRENT_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\ahaX"
!endif
!ifndef AHAX_DESKTOP_DIRECTORY
  !define AHAX_DESKTOP_DIRECTORY "$DESKTOP"
!endif
!ifndef AHAX_STARTMENU_DIRECTORY
  !define AHAX_STARTMENU_DIRECTORY "$SMPROGRAMS"
!endif

Var AhaXLegacyInstall
Var AhaXLegacyKind
Var AhaXInstallLocationChecked
Var AhaXLegacyBackup
Var AhaXShortcutUpdated
Var AhaXShortcutMigrationFailed
Var AhaXLegacyRestoreFailed

!macro AhaXInspectLegacy productKey uninstallKey displayName kind binary publisher
  ReadRegStr $0 SHCTX "${productKey}" ""
  ReadRegStr $1 SHCTX "${uninstallKey}" "MainBinaryName"
  ${If} $0 != ""
  ${AndIf} $1 == "${binary}"
    GetFullPathName $0 "$0"
    ReadRegStr $1 SHCTX "${uninstallKey}" "UninstallString"
    ${If} $1 == '$\"$0\uninstall.exe$\"'
      ReadRegStr $1 SHCTX "${uninstallKey}" "InstallLocation"
      ReadRegStr $2 SHCTX "${uninstallKey}" "DisplayName"
      ${If} $1 == '$\"$0$\"'
      ${AndIf} $2 == "${displayName}"
        ReadRegStr $1 SHCTX "${uninstallKey}" "Publisher"
        ReadRegStr $2 SHCTX "${uninstallKey}" "DisplayIcon"
        ${If} $1 == "${publisher}"
        ${AndIf} $2 == '$\"$0\${binary}$\"'
          ${If} ${FileExists} "$0\${binary}"
          ${AndIf} ${FileExists} "$0\vela.exe"
          ${AndIf} ${FileExists} "$0\uninstall.exe"
            StrCpy $AhaXLegacyInstall "$0"
            StrCpy $AhaXLegacyKind "${kind}"
          ${EndIf}
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

Function AhaXFindLegacyInstall
  Push $0
  Push $1
  Push $2
  StrCpy $AhaXLegacyInstall ""
  StrCpy $AhaXLegacyKind ""
  !insertmacro AhaXInspectLegacy "${AHAX_PREVIOUS_PRODUCT_KEY}" "${AHAX_PREVIOUS_UNINSTALL_KEY}" "AhaX" "previous" "vela.exe" "vela"
  ${If} $AhaXLegacyInstall == ""
    !insertmacro AhaXInspectLegacy "${AHAX_LEGACY_PRODUCT_KEY}" "${AHAX_LEGACY_UNINSTALL_KEY}" "Vela" "legacy" "vela.exe" "vela"
  ${EndIf}
  ${If} $AhaXLegacyInstall == ""
    ReadRegStr $0 SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "ahaXLegacyMigrationPending"
    ${If} $0 == "1"
      !insertmacro AhaXInspectLegacy "${AHAX_PRODUCT_KEY}" "${AHAX_CURRENT_UNINSTALL_KEY}" "ahaX" "pending" "ahax.exe" "ahaX"
    ${EndIf}
  ${EndIf}
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

Function AhaXValidateLegacyDeletion
  Push $0
  ReadRegStr $0 SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "MainBinaryName"
  ${If} $0 != ""
  ${AndIf} $0 != "ahax.exe"
    ${If} $0 != "vela.exe"
      SetErrorLevel 21
      Abort "旧安装登记的程序名无法确认。请先卸载原版本，或修复其安装登记后重试。"
    ${EndIf}
    ${If} $AhaXLegacyInstall != $INSTDIR
    ${AndIf} ${FileExists} "$INSTDIR\vela.exe"
      SetErrorLevel 21
      Abort "所选目录中已有无法确认归属的 vela.exe。请选择空目录或已验证的原安装目录，现有文件将保留。"
    ${EndIf}
  ${EndIf}
  Pop $0
FunctionEnd

Function AhaXBackupLegacyBinary
  ${If} $AhaXLegacyInstall != ""
  ${AndIf} $AhaXLegacyInstall == $INSTDIR
    InitPluginsDir
    StrCpy $AhaXLegacyBackup "$PLUGINSDIR\ahax-previous-binary.exe"
    ClearErrors
    CopyFiles /SILENT "$INSTDIR\vela.exe" "$AhaXLegacyBackup"
    ${If} ${Errors}
      StrCpy $AhaXLegacyBackup ""
      SetErrorLevel 22
      Abort "无法保存旧版本的临时恢复副本。请关闭旧版本后重试，现有安装将保留。"
    ${EndIf}
    ${If} $AhaXLegacyKind != "legacy"
      WriteRegStr SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "ahaXLegacyMigrationPending" "1"
    ${EndIf}
  ${EndIf}
FunctionEnd

Function AhaXRestoreLegacyBinary
  StrCpy $AhaXLegacyRestoreFailed 0
  ${If} $AhaXLegacyBackup != ""
  ${AndIf} $AhaXLegacyInstall == $INSTDIR
  ${AndIfNot} ${FileExists} "$INSTDIR\vela.exe"
    ClearErrors
    CopyFiles /SILENT "$AhaXLegacyBackup" "$INSTDIR\vela.exe"
    ${If} ${Errors}
      StrCpy $AhaXLegacyRestoreFailed 1
      DetailPrint "无法恢复旧快捷方式所需的兼容程序。请重新运行安装包，或从安装目录中的 ahax.exe 打开新版。"
    ${EndIf}
  ${EndIf}
FunctionEnd

Function AhaXRestoreLegacyInstall
  Push $0
  ${If} $AhaXInstallLocationChecked == "1"
    Pop $0
    Return
  ${EndIf}
  StrCpy $AhaXInstallLocationChecked "1"
  System::Call 'kernel32::GetCommandLineW() w.r0'
  ClearErrors
  ${GetOptions} $0 "/D=" $0
  ${IfNot} ${Errors}
    Pop $0
    Return
  ${EndIf}
  Call AhaXFindLegacyInstall
  ReadRegStr $0 SHCTX "${AHAX_PRODUCT_KEY}" ""
  ${If} $0 == ""
  ${AndIf} $AhaXLegacyInstall != ""
  ${AndIf} $INSTDIR == "${AHAX_DEFAULT_INSTALL}"
    StrCpy $INSTDIR "$AhaXLegacyInstall"
  ${EndIf}
  Pop $0
FunctionEnd

!macro AhaXRemoveLegacyRegistration
  Call AhaXFindLegacyInstall
  ${If} $AhaXLegacyInstall != ""
  ${AndIf} $AhaXLegacyInstall == $INSTDIR
    ${If} $AhaXLegacyKind == "legacy"
      DeleteRegKey SHCTX "${AHAX_LEGACY_PRODUCT_KEY}"
      DeleteRegKey SHCTX "${AHAX_LEGACY_UNINSTALL_KEY}"
    ${ElseIf} $AhaXLegacyKind == "previous"
      DeleteRegKey SHCTX "${AHAX_PREVIOUS_PRODUCT_KEY}"
    ${EndIf}
  ${EndIf}
!macroend

!macro AhaXUpdateShortcut shortcut
  StrCpy $AhaXShortcutUpdated 0
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
  ${If} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}",.r1)'
    ${If} $1 P<> 0
      ${IPersistFile::Load} $1 '("${shortcut}", ${STGM_READWRITE})i.r2'
      ${If} $2 >= 0
        ${IShellLink::SetPath} $0 '(w "$INSTDIR\ahax.exe")i.r2'
        ${If} $2 >= 0
          ${IShellLink::SetIconLocation} $0 '(w "$INSTDIR\ahax.exe", i 0)i.r2'
          ${If} $2 >= 0
            ${IPersistFile::Save} $1 '("${shortcut}",1)i.r2'
            ${If} $2 >= 0
              StrCpy $AhaXShortcutUpdated 1
            ${EndIf}
          ${EndIf}
        ${EndIf}
      ${EndIf}
      ${IUnknown::Release} $1 ""
    ${EndIf}
    ${IUnknown::Release} $0 ""
  ${EndIf}
  ${If} $AhaXShortcutUpdated = 1
    !insertmacro SetLnkAppUserModelId "${shortcut}"
    !insertmacro IsShortcutTarget "${shortcut}" "$INSTDIR\ahax.exe"
    Pop $AhaXShortcutUpdated
  ${EndIf}
  ${If} $AhaXShortcutUpdated != 1
    StrCpy $AhaXShortcutMigrationFailed 1
  ${EndIf}
!macroend

!macro AhaXMigrateShortcut old new
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  !insertmacro IsShortcutTarget "${old}" "$INSTDIR\vela.exe"
  Pop $0
  ${If} $0 = 0
    !insertmacro IsShortcutTarget "${old}" "$INSTDIR\ahax.exe"
    Pop $0
  ${EndIf}
  ${If} $0 = 1
    ${If} "${old}" == "${new}"
      !insertmacro AhaXUpdateShortcut "${old}"
      Rename "${old}" "${new}"
    ${ElseIf} ${FileExists} "${new}"
      !insertmacro IsShortcutTarget "${new}" "$INSTDIR\vela.exe"
      Pop $0
      ${If} $0 = 0
        !insertmacro IsShortcutTarget "${new}" "$INSTDIR\ahax.exe"
        Pop $0
      ${EndIf}
      ${If} $0 = 1
        !insertmacro AhaXUpdateShortcut "${new}"
        ${If} $AhaXShortcutUpdated = 1
          Delete "${old}"
        ${Else}
          !insertmacro AhaXUpdateShortcut "${old}"
        ${EndIf}
      ${Else}
        !insertmacro AhaXUpdateShortcut "${old}"
      ${EndIf}
    ${Else}
      !insertmacro AhaXUpdateShortcut "${old}"
      ${If} $AhaXShortcutUpdated = 1
        Rename "${old}" "${new}"
      ${EndIf}
    ${EndIf}
  ${EndIf}
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend
