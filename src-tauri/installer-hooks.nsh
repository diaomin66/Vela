!include "${__FILEDIR__}\installer-branding.nsh"
!define MUI_CUSTOMFUNCTION_GUIINIT AhaXRestoreLegacyInstall

!macro NSIS_HOOK_PREINSTALL
  Call AhaXRestoreLegacyInstall
  Call AhaXFindLegacyInstall
  Call AhaXValidateLegacyDeletion
  ${If} $AhaXLegacyInstall != ""
  ${AndIf} $AhaXLegacyInstall == $INSTDIR
    StrCpy $NoShortcutMode 1
    !insertmacro CheckIfAppIsRunning "$INSTDIR\vela.exe" "ahaX"
    Call AhaXBackupLegacyBinary
  ${EndIf}
  SetOutPath $INSTDIR
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $AhaXLegacyInstall != ""
  ${AndIf} $AhaXLegacyInstall == $INSTDIR
    ${IfNot} ${FileExists} "$INSTDIR\ahax.exe"
      Call AhaXRestoreLegacyBinary
      SetErrorLevel 23
      Abort "新版程序尚未完整写入，已保留旧版程序。请重新运行安装包。"
    ${EndIf}
    !insertmacro AhaXMigrateShortcut "${AHAX_DESKTOP_DIRECTORY}\Vela.lnk" "${AHAX_DESKTOP_DIRECTORY}\ahaX.lnk"
    !insertmacro AhaXMigrateShortcut "${AHAX_STARTMENU_DIRECTORY}\Vela.lnk" "${AHAX_STARTMENU_DIRECTORY}\ahaX.lnk"
    !insertmacro AhaXMigrateShortcut "${AHAX_DESKTOP_DIRECTORY}\AhaX.lnk" "${AHAX_DESKTOP_DIRECTORY}\ahaX.lnk"
    !insertmacro AhaXMigrateShortcut "${AHAX_STARTMENU_DIRECTORY}\AhaX.lnk" "${AHAX_STARTMENU_DIRECTORY}\ahaX.lnk"
    ${If} $AhaXLegacyKind == "legacy"
      DeleteRegKey SHCTX "${AHAX_LEGACY_PRODUCT_KEY}"
      DeleteRegKey SHCTX "${AHAX_LEGACY_UNINSTALL_KEY}"
    ${ElseIf} $AhaXLegacyKind == "previous"
      DeleteRegKey SHCTX "${AHAX_PREVIOUS_PRODUCT_KEY}"
    ${EndIf}
    ${If} $AhaXShortcutMigrationFailed = 1
      WriteRegStr SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "ahaXLegacyMigrationPending" "1"
      Call AhaXRestoreLegacyBinary
      ${If} $AhaXLegacyRestoreFailed = 0
        DetailPrint "部分旧快捷方式无法更新，已保留兼容程序。下次安装会继续迁移。"
      ${EndIf}
    ${Else}
      ClearErrors
      Delete "$INSTDIR\vela.exe"
      ${If} ${Errors}
        WriteRegStr SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "ahaXLegacyMigrationPending" "1"
      ${Else}
        DeleteRegValue SHCTX "${AHAX_CURRENT_UNINSTALL_KEY}" "ahaXLegacyMigrationPending"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

Function .onInstFailed
  Call AhaXRestoreLegacyBinary
FunctionEnd

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "卸载前请在 ahaX 的恢复页面恢复原配置。受管连接依赖本应用提供凭据，卸载后它们将无法继续获取 Key。连接与加密备份会保留在当前用户的数据目录。" /SD IDOK IDOK +2
    Abort
  ${EndIf}
!macroend
