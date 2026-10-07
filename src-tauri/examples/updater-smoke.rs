//! Test-only updater probe. Downloads and verifies signed artifacts, never installs.
//! This example is not linked into ahaX and does not load channel configuration.
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};
use tauri_plugin_updater::{Error, UpdaterExt};

fn main() {
    let mut args = std::env::args().skip(1);
    let endpoint: url::Url = args
        .next()
        .expect("endpoint required")
        .parse()
        .expect("valid endpoint URL");
    let report = PathBuf::from(args.next().expect("absolute report path required"));
    assert!(report.is_absolute(), "report must be an absolute path");
    let local = endpoint.scheme() == "http" && endpoint.host_str() == Some("127.0.0.1");
    let published = endpoint.as_str()
        == "https://github.com/diaomin66/ahaX/releases/latest/download/latest.json";
    assert!(
        local || published,
        "only local fixtures or the public ahaX release endpoint are supported"
    );
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "app.ahax.updater-smoke".into();
    context.config_mut().app.windows.clear();
    let config = context
        .config_mut()
        .plugins
        .0
        .get_mut("updater")
        .expect("production updater config");
    assert_eq!(
        config["requireSignedVersion"], true,
        "production config must require version-bound signatures"
    );
    if local {
        config["dangerousInsecureTransportProtocol"] = true.into();
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    let updater = handle.updater_builder()
                        .endpoints(vec![endpoint])?
                        // A smoke test may verify the currently running version.
                        // The production application uses the default strict comparator.
                        .version_comparator(|_, _| true)
                        .timeout(Duration::from_secs(30))
                        .build()?;
                    let mut update = updater.check().await?.ok_or(Error::ReleaseNotFound)?;
                    update.timeout = Some(Duration::from_secs(10 * 60));
                    let bytes = update.download(|_, _| {}, || {}).await?;
                    Ok::<_, Error>(json!({
                        "ok": true,
                        "version": update.version,
                        "currentVersion": update.current_version,
                        "downloadedBytes": bytes.len(),
                        "sha256": format!("{:x}", Sha256::digest(&bytes)),
                        "installed": false
                    }))
                }.await;
                let (value, code) = match result {
                    Ok(value) => (value, 0),
                    Err(error) => {
                        let kind = match error {
                            Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_)
                            | Error::SignedVersionMismatch { .. } | Error::MissingSignedVersion => "signature",
                            _ => "network-or-metadata",
                        };
                        (json!({"ok": false, "errorKind": kind, "error": error.to_string(), "installed": false}), 1)
                    }
                };
                let code = match std::fs::write(report, serde_json::to_vec_pretty(&value).unwrap()) {
                    Ok(()) => code,
                    Err(_) => 2,
                };
                handle.exit(code);
            });
            Ok(())
        })
        .run(context)
        .expect("unable to run isolated updater probe");
}
