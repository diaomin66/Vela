use axum::{body::Body, extract::State, http::{header, Request, Response, StatusCode}, Router};
use serde::Serialize;
use std::{collections::HashMap, sync::{Arc, Mutex}};
use tokio::{net::TcpListener, sync::Mutex as AsyncMutex, task::JoinHandle};
use uuid::Uuid;

const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
const PREVIEW_POLICY: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' blob:; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; connect-src 'none'; frame-src 'none'; object-src 'none'; worker-src 'none'; base-uri 'none'; form-action 'none'; sandbox allow-scripts";

#[derive(Clone)]
struct PreviewDocuments {
    host: String,
    capability: String,
    documents: Arc<Mutex<HashMap<String, String>>>,
}

struct PreviewServer {
    state: PreviewDocuments,
    task: JoinHandle<()>,
}

impl Drop for PreviewServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
pub struct ArtifactPreviewState {
    server: AsyncMutex<Option<PreviewServer>>,
}

#[derive(Serialize)]
pub struct ArtifactLocation {
    id: String,
    url: String,
}

impl ArtifactPreviewState {
    pub async fn start() -> Result<(Self, String), String> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await
            .map_err(|_| "无法启动本机作品预览。".to_owned())?;
        let port = listener.local_addr().map_err(|_| "无法读取作品预览端口。".to_owned())?.port();
        let state = PreviewDocuments {
            host: format!("127.0.0.1:{port}"),
            capability: Uuid::new_v4().simple().to_string(),
            documents: Arc::default(),
        };
        let frame_source = format!("http://{}/{}/", state.host, state.capability);
        let router = Router::new().fallback(serve_document).with_state(state.clone());
        let task = tokio::spawn(async move { let _ = axum::serve(listener, router).await; });
        Ok((Self { server: AsyncMutex::new(Some(PreviewServer { state, task })) }, frame_source))
    }

    async fn create(&self, html: String) -> Result<ArtifactLocation, String> {
        if html.len() > MAX_DOCUMENT_BYTES {
            return Err("预览内容超过 2 MB，请下载原文查看。".into());
        }
        let server = self.server.lock().await;
        let state = &server.as_ref().ok_or_else(|| "本机作品预览未启动，请重新打开 AhaX。".to_owned())?.state;
        let mut documents = state.documents.lock().map_err(|_| "作品预览暂不可用。".to_owned())?;
        if documents.len() >= 128 || documents.values().map(String::len).sum::<usize>() + html.len() > MAX_TOTAL_BYTES {
            return Err("当前预览较多，请关闭部分作品后重试。".into());
        }
        let id = Uuid::new_v4().simple().to_string();
        let url = format!("http://{}/{}/{}.html", state.host, state.capability, id);
        documents.insert(id.clone(), html);
        Ok(ArtifactLocation { id, url })
    }

    async fn release(&self, id: &str) {
        if let Some(server) = self.server.lock().await.as_ref() {
            if let Ok(mut documents) = server.state.documents.lock() {
                documents.remove(id);
            }
        }
    }
}

async fn serve_document(State(state): State<PreviewDocuments>, request: Request<Body>) -> Response<Body> {
    let expected_prefix = format!("/{}/", state.capability);
    let id = request.uri().path().strip_prefix(&expected_prefix).and_then(|value| value.strip_suffix(".html"));
    let valid_host = request.headers().get(header::HOST).and_then(|value| value.to_str().ok()) == Some(state.host.as_str());
    let document = if request.method() == axum::http::Method::GET && valid_host && request.uri().query().is_none() {
        id.and_then(|id| state.documents.lock().ok()?.get(id).cloned())
    } else { None };
    let status = if document.is_some() { StatusCode::OK } else { StatusCode::NOT_FOUND };
    Response::builder().status(status)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CONTENT_SECURITY_POLICY, PREVIEW_POLICY)
        .header(header::CACHE_CONTROL, "no-store")
        .header("referrer-policy", "no-referrer")
        .header("x-content-type-options", "nosniff")
        .header("permissions-policy", "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), clipboard-read=(), clipboard-write=()")
        .body(Body::from(document.unwrap_or_default())).unwrap()
}

#[cfg(not(test))]
#[tauri::command]
pub async fn create_artifact_preview(state: tauri::State<'_, ArtifactPreviewState>, html: String) -> Result<ArtifactLocation, String> {
    state.create(html).await
}

#[cfg(not(test))]
#[tauri::command]
pub async fn release_artifact_preview(state: tauri::State<'_, ArtifactPreviewState>, id: String) -> Result<(), String> {
    state.release(&id).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn preview_is_a_capability_scoped_read_only_document_server() {
        let (state, frame_source) = ArtifactPreviewState::start().await.unwrap();
        let html = "<!doctype html><script>window.animation = true</script><svg></svg>";
        let location = state.create(html.into()).await.unwrap();
        assert!(location.url.starts_with(&frame_source));
        assert!(!frame_source.contains('*'));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client.get(&location.url).send().await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-security-policy"], PREVIEW_POLICY);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(!response.headers().contains_key("access-control-allow-origin"));
        assert_eq!(response.text().await.unwrap(), html);
        assert_eq!(client.post(&location.url).body("replace").send().await.unwrap().status(), 404);
        assert_eq!(client.get(format!("{}?secret=value", location.url)).send().await.unwrap().status(), 404);
        assert_eq!(client.get(&location.url).header("host", "attacker.example").send().await.unwrap().status(), 404);
        let unknown = location.url.replace(&location.id, &Uuid::new_v4().simple().to_string());
        assert_eq!(client.get(unknown).send().await.unwrap().status(), 404);
        state.release(&location.id).await;
        assert_eq!(client.get(&location.url).send().await.unwrap().status(), 404);
    }
}
