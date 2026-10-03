!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "卸载前请在 Vela 的恢复页面恢复原配置。受管连接依赖本应用提供凭据，卸载后它们将无法继续获取 Key。连接与加密备份会保留在当前用户的数据目录。" /SD IDOK IDOK +2
    Abort
  ${EndIf}
!macroend
