//! Headless credential-helper mode, dispatched before constructing any GUI runtime.
use super::runtime::effective_gateway_port;
use crate::{
    core::{self, AppPaths},
    security,
};
use zeroize::{Zeroize, Zeroizing};

/// Runs before Tauri startup, so credentials can be read even while the app is closed.
/// No window, app store, configuration read, or network request is involved.
pub fn run_credential_mode() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--gateway-credential") {
        if args.len() != 2 {
            std::process::exit(2);
        }
        let outcome = (|| -> Result<(), String> {
            let paths = AppPaths::discover_identity()?;
            let token = Zeroizing::new(security::gateway_token(&paths)?);
            let port = effective_gateway_port(&paths, &core::load_store(&paths)?);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "无法启动网关检查。")?;
            runtime.block_on(async {
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .retry(reqwest::retry::never())
                    .timeout(std::time::Duration::from_millis(500))
                    .build()
                    .map_err(|_| "无法检查本地网关。")?;
                let health = format!("http://127.0.0.1:{port}/health");
                let mut launched = false;
                for _ in 0..40 {
                    if let Ok(response) =
                        client.get(&health).bearer_auth(token.as_str()).send().await
                    {
                        if response.status().is_success() {
                            if let Ok(body) = response.json::<serde_json::Value>().await {
                                if matches!(body.get("service").and_then(|v| v.as_str()), Some("ahaX" | "Vela"))
                                    && body.get("running").and_then(|v| v.as_bool()) == Some(true)
                                {
                                    return Ok(());
                                }
                            }
                        }
                    }
                    if !launched {
                        let mut command = std::process::Command::new(&paths.helper);
                        command.arg("--background");
                        #[cfg(windows)]
                        {
                            use std::os::windows::process::CommandExt;
                            command.creation_flags(0x08000000);
                        }
                        command
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn()
                            .map_err(|_| "无法启动后台网关。")?;
                        launched = true;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                Err::<(), String>("本地网关未就绪。".into())
            })?;
            use std::io::Write;
            std::io::stdout()
                .write_all(token.as_bytes())
                .and_then(|_| std::io::stdout().flush())
                .map_err(|_| "无法输出网关凭据。")?;
            Ok(())
        })();
        if outcome.is_err() {
            std::process::exit(4);
        }
        return true;
    }
    if args.get(1).map(String::as_str) != Some("--credential") {
        return false;
    }
    if args.len() != 3 {
        std::process::exit(2);
    }
    match security::get_secret(&args[2]) {
        Ok(mut secret) => {
            use std::io::Write;
            // Windows supplies inherited pipe handles even for a GUI-subsystem executable.
            let mut stdout = std::io::stdout();
            let output = stdout
                .write_all(secret.as_bytes())
                .and_then(|_| stdout.flush());
            secret.zeroize();
            if output.is_err() {
                std::process::exit(3);
            }
        }
        Err(_) => std::process::exit(4),
    }
    true
}
