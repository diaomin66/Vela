!include "${__FILEDIR__}\installer-branding.nsh"
!define MUI_CUSTOMFUNCTION_GUIINIT AhaXRestoreLegacyInstall

!macro NSIS_HOOK_PREINSTALL
  Call AhaXRestoreLegacyInstall
  Call AhaXFindLegacyInstall
  ${If} $AhaXLegacyInstall != ""
  ${AndIf} $AhaXLegacyInstall == $INSTDIR
    StrCpy $NoShortcutMode 1
  ${EndIf}
  SetOutPath $INSTDIR
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Call AhaXFindLegacyInstall
  ${If} $AhaXLegacyInstall != ""
  ${AndIf} $AhaXLegacyInstall == $INSTDIR
    !insertmacro AhaXMigrateShortcut "$DESKTOP\Vela.lnk" "$DESKTOP\AhaX.lnk"
    !insertmacro AhaXMigrateShortcut "$SMPROGRAMS\Vela.lnk" "$SMPROGRAMS\AhaX.lnk"
    !insertmacro AhaXRemoveLegacyRegistration
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "卸载前请在 AhaX 的恢复页面恢复原配置。受管连接依赖本应用提供凭据，卸载后它们将无法继续获取 Key。连接与加密备份会保留在当前用户的数据目录。" /SD IDOK IDOK +2
    Abort
  ${EndIf}
!macroend
