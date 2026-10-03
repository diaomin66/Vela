//! Native desktop integration: installed-app discovery, launch, tray, and window state.
use super::state::AppState;
use serde::Deserialize;
use std::sync::Mutex;
use tauri::Manager;

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopInfo {
    pub(super) executable: Option<String>,
    pub(super) app_id: Option<String>,
    pub(super) version: Option<String>,
}
impl DesktopInfo {
    pub(super) fn installed(&self) -> bool {
        self.executable.is_some() || self.app_id.is_some()
    }
}

pub(super) fn desktop_info() -> DesktopInfo {
    static CACHE: std::sync::OnceLock<Mutex<Option<(std::time::Instant, DesktopInfo)>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(entry) = cache.lock() {
        if let Some((time, info)) = entry.as_ref() {
            if time.elapsed() < std::time::Duration::from_secs(60) {
                return info.clone();
            }
        }
    }
    let info = detect_desktop();
    if let Ok(mut entry) = cache.lock() {
        *entry = Some((std::time::Instant::now(), info.clone()));
    }
    info
}
fn detect_desktop() -> DesktopInfo {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Fixed script: neither paths, profiles, nor credentials are interpolated into PowerShell.
        let output = std::process::Command::new("powershell.exe").creation_flags(0x08000000)
            .args(["-NoProfile", "-NonInteractive", "-Command", "$p = Get-AppxPackage -Name '*Codex*' -ErrorAction SilentlyContinue | Select-Object -First 1; if ($p) { $m = $p | Get-AppxPackageManifest; $a = $m.Package.Applications.Application | Select-Object -First 1; @{ appId = ($p.PackageFamilyName + '!' + $a.Id); version = $p.Version.ToString() } | ConvertTo-Json -Compress }"])
            .output();
        if let Ok(output) = output {
            if let Ok(info) = serde_json::from_slice::<DesktopInfo>(&output.stdout) {
                if info.installed() {
                    return info;
                }
            }
        }
        for root in [
            std::env::var_os("LOCALAPPDATA"),
            std::env::var_os("ProgramFiles"),
        ]
        .into_iter()
        .flatten()
        {
            for relative in ["Programs/Codex/Codex.exe", "Codex/Codex.exe"] {
                let candidate = std::path::PathBuf::from(&root).join(relative);
                if candidate.is_file() {
                    return DesktopInfo {
                        executable: Some(candidate.to_string_lossy().into()),
                        app_id: None,
                        version: None,
                    };
                }
            }
        }
    }
    DesktopInfo::default()
}
pub(super) async fn open_codex() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let info = desktop_info();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            if let Some(id) = info.app_id {
                std::process::Command::new("explorer.exe")
                    .arg(format!("shell:AppsFolder\\{id}"))
                    .creation_flags(0x08000000)
                    .spawn()
                    .map_err(|_| "无法打开 Codex，请从开始菜单打开。")?;
                return Ok(
                    "已请求打开 Codex。若它已在运行，请完全退出后重新打开以加载新配置。".into(),
                );
            }
            if let Some(executable) = info.executable {
                std::process::Command::new(executable)
                    .spawn()
                    .map_err(|_| "无法打开 Codex，请从开始菜单打开。")?;
                return Ok("已打开 Codex。配置生效情况请通过实际任务确认。".into());
            }
        }
        let _ = info;
        Err("未检测到官方 Codex 桌面版。请先安装，再重新检测。".into())
    })
    .await
    .map_err(|_| "无法完成启动请求。".to_string())?
}

pub(super) fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::TrayIconBuilder,
    };
    let open = MenuItem::with_id(app, "open", "打开 Vela", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Vela", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .tooltip("Vela · 本地模型网关")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "quit" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    if let Some(mut gateway) = state.gateway.lock().await.take() {
                        gateway.shutdown().await;
                    }
                    handle.exit(0);
                });
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    if std::env::args().any(|a| a == "--background") {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
    } else if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }

    Ok(())
}

pub(super) fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
