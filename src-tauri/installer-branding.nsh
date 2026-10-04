!ifndef AHAX_LEGACY_PRODUCT_KEY
  !define AHAX_LEGACY_PRODUCT_KEY "Software\vela\Vela"
!endif
!ifndef AHAX_LEGACY_UNINSTALL_KEY
  !define AHAX_LEGACY_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vela"
!endif
!ifndef AHAX_PRODUCT_KEY
  !define AHAX_PRODUCT_KEY "Software\vela\AhaX"
!endif
!ifndef AHAX_DEFAULT_INSTALL
  !define AHAX_DEFAULT_INSTALL "$LOCALAPPDATA\AhaX"
!endif

Var AhaXLegacyInstall
Var AhaXInstallLocationChecked

Function AhaXFindLegacyInstall
  Push $0
  Push $1
  Push $2
  StrCpy $AhaXLegacyInstall ""
  ReadRegStr $0 SHCTX "${AHAX_LEGACY_PRODUCT_KEY}" ""
  ReadRegStr $1 SHCTX "${AHAX_LEGACY_UNINSTALL_KEY}" "MainBinaryName"
  ${If} $0 != ""
  ${AndIf} $1 == "vela.exe"
    GetFullPathName $0 "$0"
    ReadRegStr $1 SHCTX "${AHAX_LEGACY_UNINSTALL_KEY}" "UninstallString"
    ${If} $1 == '$\"$0\uninstall.exe$\"'
      ReadRegStr $1 SHCTX "${AHAX_LEGACY_UNINSTALL_KEY}" "InstallLocation"
      ReadRegStr $2 SHCTX "${AHAX_LEGACY_UNINSTALL_KEY}" "DisplayName"
      ${If} $1 == '$\"$0$\"'
      ${AndIf} $2 == "Vela"
        ReadRegStr $1 SHCTX "${AHAX_LEGACY_UNINSTALL_KEY}" "Publisher"
        ReadRegStr $2 SHCTX "${AHAX_LEGACY_UNINSTALL_KEY}" "DisplayIcon"
        ${If} $1 == "vela"
        ${AndIf} $2 == '$\"$0\vela.exe$\"'
          ${If} ${FileExists} "$0\vela.exe"
          ${AndIf} ${FileExists} "$0\uninstall.exe"
            StrCpy $AhaXLegacyInstall "$0"
          ${EndIf}
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
  Pop $2
  Pop $1
  Pop $0
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
    DeleteRegKey SHCTX "${AHAX_LEGACY_PRODUCT_KEY}"
    DeleteRegKey SHCTX "${AHAX_LEGACY_UNINSTALL_KEY}"
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
  ${If} $0 = 1
    ${If} ${FileExists} "${new}"
      !insertmacro IsShortcutTarget "${new}" "$INSTDIR\vela.exe"
      Pop $0
      ${If} $0 = 1
        !insertmacro SetLnkAppUserModelId "${new}"
        Delete "${old}"
      ${EndIf}
    ${Else}
      ClearErrors
      Rename "${old}" "${new}"
      ${IfNot} ${Errors}
        !insertmacro SetLnkAppUserModelId "${new}"
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
