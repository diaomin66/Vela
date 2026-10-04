//! Minimal local app-server transport. No turn, authentication, or deletion RPCs.
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const MAX_REPLY: u64 = 16 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

pub(super) struct Client {
    child: Child,
    input: ChildStdin,
    replies: Option<Receiver<Result<Value, String>>>,
    reader: Option<std::thread::JoinHandle<()>>,
    sequence: u64,
    deadline: Instant,
    last_rpc_code: Option<i64>,
}

impl Client {
    pub(super) fn start(
        executable: &Path,
        root: &Path,
        sqlite_root: Option<&Path>,
    ) -> Result<Self, String> {
        let mut command = Command::new(executable);
        command
            .arg("app-server")
            .args(["-c", "experimental_thread_store.type=\"local\""])
            .env("CODEX_HOME", root)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // A previous source must never inherit the current source's custom DB.
        command.env_remove("CODEX_SQLITE_HOME");
        if let Some(sqlite_root) = sqlite_root {
            command.env("CODEX_SQLITE_HOME", sqlite_root);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|_| "无法启动本机线程服务，请检查官方客户端是否完整安装。")?;
        let input = child.stdin.take().ok_or("无法连接线程服务。")?;
        let output = child.stdout.take().ok_or("无法读取线程服务响应。")?;
        let (sender, receiver) = mpsc::sync_channel(4);
        let reader = std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let result = (&mut output)
                    .take(MAX_REPLY + 1)
                    .read_until(b'\n', &mut bytes);
                let message = match result {
                    Ok(0) => break,
                    Ok(_) if bytes.len() as u64 > MAX_REPLY => {
                        Err("线程服务响应超过安全读取范围。".into())
                    }
                    Ok(_) => serde_json::from_slice(&bytes)
                        .map_err(|_| "本机线程服务返回了不兼容的协议。".into()),
                    Err(_) => Err("本机线程服务连接中断。".into()),
                };
                let failed = message.is_err();
                if sender.send(message).is_err() || failed {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            replies: Some(receiver),
            reader: Some(reader),
            sequence: 0,
            deadline: Instant::now() + Duration::from_secs(300),
            last_rpc_code: None,
        };
        let initialized = client.call("initialize", json!({"clientInfo":{"name":"ahax_threads","title":"AhaX thread protection","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":false}}))?;
        if let Some(actual) = initialized.get("codexHome").and_then(Value::as_str) {
            if path_identity(Path::new(actual))? != path_identity(root)? {
                return Err("线程服务打开了不同的来源目录，已停止重新索引。".into());
            }
        }
        client.notify("initialized", json!({}))?;
        Ok(client)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({"method":method,"params":params}))
    }
    fn send(&mut self, message: &Value) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(message).map_err(|_| "无法准备线程服务请求。")?;
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .and_then(|_| self.input.flush())
            .map_err(|_| "本机线程服务连接已关闭。".into())
    }
    pub(super) fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.last_rpc_code = None;
        self.sequence += 1;
        let id = self.sequence;
        self.send(&json!({"id":id,"method":method,"params":params}))?;
        let request_deadline = self.deadline.min(Instant::now() + REQUEST_TIMEOUT);
        loop {
            let remaining = request_deadline
                .checked_duration_since(Instant::now())
                .ok_or("线程索引处理超时；原日志和保护快照已保留，可稍后重试。")?;
            let response = self
                .replies
                .as_ref()
                .ok_or("线程服务已关闭。")?
                .recv_timeout(remaining)
                .map_err(|_| "线程索引处理超时或服务已退出；原日志和保护快照已保留。")??;
            // Server requests are not needed for listing. Decline explicitly;
            // never leave a hidden approval request waiting in the child.
            if response.get("method").is_some() {
                if let Some(request_id) = response.get("id") {
                    self.send(&json!({"id":request_id,"error":{"code":-32601,"message":"This client only supports local thread listing"}}))?;
                }
                continue;
            }
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if response.get("error").is_some() {
                self.last_rpc_code = response.pointer("/error/code").and_then(Value::as_i64);
                // Upstream messages may contain conversation content or paths.
                return Err("本机线程服务不支持此索引请求，或当前配置无法加载。请更新官方客户端并通过诊断检查配置。".into());
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| "本机线程服务的响应缺少结果。".into());
        }
    }

    pub(super) fn list_all(&mut self, archived: bool) -> Result<u64, String> {
        let mut cursor: Option<String> = None;
        let mut seen = std::collections::HashSet::new();
        let mut count = 0;
        let mut includes_app_server = true;
        for _ in 0..2000 {
            let mut params = json!({"cursor":cursor,"limit":100,"archived":archived,"modelProviders":[],
                "sourceKinds":["cli","vscode","exec","appServer","subAgent","subAgentReview","subAgentCompact","subAgentThreadSpawn","subAgentOther","unknown"],
                "useStateDbOnly":false});
            if !includes_app_server {
                params["sourceKinds"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|value| value != "appServer");
            }
            let reply = self.call("thread/list", params.clone());
            let result = match reply {
                Err(_) if self.last_rpc_code == Some(-32602) && includes_app_server => {
                    // Older public schemas predate the appServer source kind.
                    includes_app_server = false;
                    params["sourceKinds"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|value| value != "appServer");
                    self.call("thread/list", params)?
                }
                result => result?,
            };
            let data = result
                .get("data")
                .and_then(Value::as_array)
                .ok_or("线程列表格式不兼容。")?;
            for thread in data {
                if let Some(id) = thread.get("id").and_then(Value::as_str) {
                    seen.insert(id.to_owned());
                }
            }
            let next = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            count += 1;
            if next.is_none() {
                return Ok(seen.len() as u64);
            }
            if next == cursor || count >= 2000 {
                return Err("线程列表未完整返回，保护快照已保留，请缩小来源后重试。".into());
            }
            cursor = next;
        }
        Err("线程列表超出单次处理范围。".into())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        drop(self.replies.take());
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn path_identity(path: &Path) -> Result<String, String> {
    let resolved = std::fs::canonicalize(path).map_err(|_| "无法确认线程来源目录。")?;
    let text = resolved.to_string_lossy().into_owned();
    #[cfg(windows)]
    let text = text.to_lowercase();
    Ok(text)
}

pub(super) fn discover() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    for root in [
        std::env::var_os("LOCALAPPDATA"),
        std::env::var_os("ProgramFiles"),
    ]
    .into_iter()
    .flatten()
    {
        for relative in [
            "Programs/Codex/resources/codex.exe",
            "Codex/resources/codex.exe",
            "Programs/Codex/app/resources/codex.exe",
        ] {
            candidates.push(PathBuf::from(&root).join(relative));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A fixed script discovers installed package roots; no user input is
        // interpolated and no shell runs the selected binary.
        let mut child = Command::new("powershell.exe").creation_flags(0x08000000)
            .args(["-NoProfile","-NonInteractive","-Command","Get-AppxPackage -Name '*Codex*' -ErrorAction SilentlyContinue | ForEach-Object { $_.InstallLocation }"])
            .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok();
        if let Some(child) = &mut child {
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
            if let Some(stdout) = child.stdout.take() {
                let mut bytes = Vec::new();
                let _ = stdout.take(65536).read_to_end(&mut bytes);
                for root in String::from_utf8_lossy(&bytes)
                    .lines()
                    .map(str::trim)
                    .filter(|root| !root.is_empty())
                {
                    for relative in ["app/resources/codex.exe", "resources/codex.exe"] {
                        candidates.push(PathBuf::from(root).join(relative));
                    }
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path).filter(|directory| directory.is_absolute()) {
            candidates.push(directory.join(if cfg!(windows) { "codex.exe" } else { "codex" }));
        }
    }
    candidates.into_iter().find(|path| path.is_file()).ok_or_else(|| "未找到官方客户端附带的本地线程服务。请更新或安装官方客户端后重试；现有线程保护不受影响。".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::{core::AppPaths, threads::ThreadState};

    #[test]
    #[ignore = "requires AHAX_TEST_APP_SERVER pointing to an explicitly selected official executable"]
    fn official_service_reads_restored_history_in_an_isolated_home() {
        let executable = PathBuf::from(
            std::env::var_os("AHAX_TEST_APP_SERVER").expect("set AHAX_TEST_APP_SERVER"),
        );
        assert!(executable.is_absolute() && executable.is_file());
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("home");
        let active = root.join("sessions/2026/10/04");
        let archive = root.join("archived_sessions");
        std::fs::create_dir_all(&active).unwrap();
        std::fs::create_dir_all(&archive).unwrap();
        std::fs::write(
            root.join("config.toml"),
            "# Isolated local thread interoperability fixture.\n",
        )
        .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let archived_id = uuid::Uuid::new_v4().to_string();
        let marker = "AhaX isolated historical message";
        let bytes = fixture(&id, &root, marker);
        let source = active.join(format!("rollout-2026-10-04T09-00-00-{id}.jsonl"));
        std::fs::write(&source, &bytes).unwrap();
        std::fs::write(
            archive.join(format!("rollout-2026-10-04T09-00-00-{archived_id}.jsonl")),
            fixture(&archived_id, &root, "Archived fixture"),
        )
        .unwrap();
        let paths = AppPaths {
            data: sandbox.path().join("data"),
            config: root.join("config.toml"),
            helper: sandbox.path().join("unused.exe"),
        };
        let state = ThreadState::new(paths);
        let protected = state.start_scan().unwrap();
        let key = protected
            .threads
            .iter()
            .find(|thread| thread.thread_id == id)
            .unwrap()
            .key
            .clone();
        assert_eq!(protected.protected, 2);
        std::fs::remove_file(&source).unwrap();
        let missing = state.start_scan().unwrap();
        assert_eq!(missing.recoverable, 1);
        let preview = state.preview_restore(key.clone()).unwrap();
        state.restore(key, preview.expected_hash).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        let mut client = Client::start(&executable, &root, None).unwrap();
        assert_eq!(client.list_all(false).unwrap(), 1);
        assert_eq!(client.list_all(true).unwrap(), 1);
        let thread = client
            .call("thread/read", json!({"threadId":id,"includeTurns":true}))
            .unwrap();
        assert_eq!(
            thread.pointer("/thread/id").and_then(Value::as_str),
            Some(id.as_str())
        );
        assert!(serde_json::to_string(&thread).unwrap().contains(marker));
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
    }

    fn fixture(id: &str, root: &Path, message: &str) -> Vec<u8> {
        let values = [
            json!({"timestamp":"2026-10-04T09:00:00Z","type":"session_meta","payload":{"id":id,"timestamp":"2026-10-04T09:00:00Z","cwd":root,"originator":"codex_cli_rs","cli_version":"0.160.0","source":"cli","model_provider":"openai"}}),
            json!({"timestamp":"2026-10-04T09:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":message}]}}),
            json!({"timestamp":"2026-10-04T09:00:01Z","type":"event_msg","payload":{"type":"user_message","message":message,"images":[],"local_images":[],"text_elements":[]}}),
            json!({"timestamp":"2026-10-04T09:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Retained answer."}]}}),
        ];
        let mut bytes = Vec::new();
        for value in values {
            bytes.extend(serde_json::to_vec(&value).unwrap());
            bytes.push(b'\n');
        }
        bytes
    }
}
