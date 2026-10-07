use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const TOKEN: &str = "ahax-test-local-credential-01234567890123456789";
const KEY: &str = "sk-private-channel-credential";
const PROFILE_A: &str = "bcaa10f8-9c32-4a16-98ec-15ef8cbb23f8";
const PROFILE_B: &str = "047d02b9-3c8c-4abc-9cb4-daf425ad7b78";

struct MockServer {
    base: String,
    task: JoinHandle<()>,
}
impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn mock(router: Router) -> MockServer {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    MockServer {
        base: format!("http://127.0.0.1:{port}"),
        task,
    }
}
fn route(base: &str, alias: &str, profile: &str) -> GatewayRoute {
    GatewayRoute {
        route_id: alias.into(),
        display_name: "测试渠道（gpt-test）".into(),
        profile_id: profile.into(),
        upstream_model: "gpt-test".into(),
        base_url: base.into(),
    }
}
async fn gateway(entries: Vec<GatewayRoute>) -> GatewayHandle {
    start_with_secrets(
        GatewayCatalog { entries },
        TOKEN.into(),
        0,
        Arc::new(|_| Ok(KEY.into())),
    )
    .await
    .unwrap()
}
fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}
fn url(gateway: &GatewayHandle, path: &str) -> String {
    format!("http://127.0.0.1:{}{path}", gateway.status().port)
}
fn post(gateway: &GatewayHandle, alias: &str, streaming: bool) -> reqwest::RequestBuilder {
    client()
        .post(url(gateway, "/v1/responses"))
        .bearer_auth(TOKEN)
        .json(&json!({"model":alias,"input":"hello","stream":streaming}))
}

#[tokio::test]
async fn models_and_health_require_local_auth_and_reject_browser_or_spoofed_host() {
    let mut service = gateway(vec![route("https://example.test/v1", "route-a", PROFILE_A)]).await;
    let client = client();
    assert_eq!(
        client
            .get(url(&service, "/v1/models"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(url(&service, "/health"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for (header, value) in [
        ("origin", "https://example.test"),
        ("host", "attacker.test"),
    ] {
        assert_eq!(
            client
                .get(url(&service, "/v1/models"))
                .bearer_auth(TOKEN)
                .header(header, value)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    let health: Value = client
        .get(url(&service, "/health"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["service"], "ahaX");
    let response = client
        .get(url(&service, "/v1/models"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert!(!response
        .headers()
        .contains_key("access-control-allow-origin"));
    let text = response.text().await.unwrap();
    assert!(!text.contains(KEY));
    assert!(!text.contains(PROFILE_A));
    assert!(!text.contains("example.test"));
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["data"][0]["id"], "route-a");
    assert_eq!(value["data"][0]["display_name"], "测试渠道（gpt-test）");
    service.shutdown().await;
    assert!(!service.status().running);
}

#[tokio::test]
async fn identical_models_on_two_channels_route_exactly_and_keep_public_alias() {
    async fn handler(
        State(label): State<&'static str>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response<Body> {
        assert_eq!(headers[header::AUTHORIZATION], format!("Bearer {KEY}"));
        assert!(!headers.contains_key("cookie"));
        assert!(!headers.contains_key("x-forwarded-for"));
        assert_eq!(headers["session_id"], "session-123");
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["model"], "gpt-test");
        assert_eq!(value["input"], "hello");
        json_response(
            StatusCode::OK,
            json!({"object":"response","id":format!("resp_{label}"),"model":"gpt-test","output":label}),
        )
    }
    let a = mock(
        Router::new()
            .route("/v1/responses", any(handler))
            .with_state("a"),
    )
    .await;
    let b = mock(
        Router::new()
            .route("/responses", any(handler))
            .with_state("b"),
    )
    .await;
    let mut service = gateway(vec![
        route(&format!("{}/v1", a.base), "route-a", PROFILE_A),
        route(&b.base, "route-b", PROFILE_B),
    ])
    .await;
    for (alias, expected) in [("route-a", "a"), ("route-b", "b")] {
        let response = post(&service, alias, false)
            .header("session_id", "session-123")
            .header("cookie", "must-not-forward")
            .header("x-forwarded-for", "1.2.3.4")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value = response.json().await.unwrap();
        assert_eq!(value["model"], alias);
        assert_eq!(value["output"], expected);
    }
    assert_eq!(
        post(&service, "missing-route", false)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client()
            .post(url(&service, "/v1/chat/completions"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    service.shutdown().await;
}

#[tokio::test]
async fn continuation_is_bound_to_route_and_unknown_continuation_is_rejected() {
    let upstream = mock(Router::new().route(
        "/responses",
        any(|| async {
            json_response(
                StatusCode::OK,
                json!({"id":"resp_first","object":"response","model":"gpt-test"}),
            )
        }),
    ))
    .await;
    let mut service = gateway(vec![
        route(&upstream.base, "route-a", PROFILE_A),
        route(&upstream.base, "route-b", PROFILE_B),
    ])
    .await;
    assert!(post(&service, "route-a", false)
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    for (alias, previous, expected) in [
        ("route-a", "resp_first", StatusCode::OK),
        ("route-b", "resp_first", StatusCode::CONFLICT),
        ("route-a", "resp_unknown", StatusCode::CONFLICT),
    ] {
        let response = client()
            .post(url(&service, "/responses"))
            .bearer_auth(TOKEN)
            .json(&json!({"model":alias,"previous_response_id":previous,"input":[]}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    service.shutdown().await;
}

#[tokio::test]
async fn streaming_arrives_before_completion_rewrites_model_and_sanitizes_error_key() {
    let upstream = mock(Router::new().route("/responses", any(|| async {
        let chunks = stream::unfold(0, |stage| async move {
            let chunk = match stage {
                0 => "event: response.created\r\ndata: {\"type\":\"response.created\",\"response\":{\"object\":\"response\",\"id\":\"resp_sse\",\"model\":\"gpt-test\"}}\r\n\r\n".to_string(),
                1 => { tokio::time::sleep(Duration::from_millis(250)).await; format!("event: error\ndata: {{\"type\":\"error\",\"message\":\"bad key {KEY}\"}}\n\n") },
                2 => "data: [DONE]\n\n".into(),
                _ => return None,
            };
            Some((Ok::<Bytes, Infallible>(Bytes::from(chunk)),stage+1))
        });
        Response::builder().header(header::CONTENT_TYPE,"text/event-stream").body(Body::from_stream(chunks)).unwrap()
    }))).await;
    let mut service = gateway(vec![route(&upstream.base, "route-a", PROFILE_A)]).await;
    let response = post(&service, "route-a", true).send().await.unwrap();
    let mut chunks = response.bytes_stream();
    let first = tokio::time::timeout(Duration::from_millis(150), chunks.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mut all = String::from_utf8(first.to_vec()).unwrap();
    assert!(all.contains("route-a"));
    assert!(!all.contains("gpt-test"));
    while let Some(chunk) = chunks.next().await {
        all.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
    }
    assert!(!all.contains(KEY));
    assert!(all.contains("[redacted]"));
    assert!(all.contains("[DONE]"));
    assert!(service.state.continuations.lock().unwrap().matches(
        "resp_sse",
        &service.state.catalog.read().unwrap().entries[0]
    ));
    service.shutdown().await;
}

#[tokio::test]
async fn failed_or_redirected_post_is_not_retried_and_never_leaks_upstream_errors() {
    let hits = Arc::new(AtomicUsize::new(0));
    let sink_hits = Arc::new(AtomicUsize::new(0));
    let sink_count = sink_hits.clone();
    let sink = mock(Router::new().fallback(any(move || {
        let count = sink_count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            "unexpected"
        }
    })))
    .await;
    let destination = sink.base.clone();
    let count = hits.clone();
    let upstream = mock(Router::new().route(
        "/responses",
        any(move || {
            let destination = destination.clone();
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header(header::LOCATION, destination)
                    .body(Body::from(KEY))
                    .unwrap()
            }
        }),
    ))
    .await;
    let mut service = gateway(vec![route(&upstream.base, "route-a", PROFILE_A)]).await;
    let response = post(&service, "route-a", false).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(!response.text().await.unwrap().contains(KEY));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(sink_hits.load(Ordering::SeqCst), 0);
    service.shutdown().await;
}

struct CancelGuard(Arc<AtomicBool>);
impl Drop for CancelGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn disconnecting_codex_cancels_the_upstream_stream() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let indicator = cancelled.clone();
    let upstream = mock(Router::new().route(
        "/responses",
        any(move || {
            let guard = CancelGuard(indicator.clone());
            async move {
                let chunks = stream::unfold(guard, |guard| async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    Some((
                        Ok::<Bytes, Infallible>(Bytes::from_static(
                            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n",
                        )),
                        guard,
                    ))
                });
                Response::builder()
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .body(Body::from_stream(chunks))
                    .unwrap()
            }
        }),
    ))
    .await;
    let mut service = gateway(vec![route(&upstream.base, "route-a", PROFILE_A)]).await;
    let mut response = post(&service, "route-a", true).send().await.unwrap();
    response.chunk().await.unwrap().unwrap();
    drop(response);
    tokio::time::timeout(Duration::from_secs(3), async {
        while !cancelled.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("upstream request must be cancelled after downstream disconnect");
    service.shutdown().await;
}

#[tokio::test]
async fn compact_is_forwarded_and_catalog_updates_cannot_reassign_a_route() {
    let upstream = mock(Router::new().route(
        "/responses/compact",
        any(|body: Bytes| async move {
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["model"], "gpt-test");
            json_response(
                StatusCode::OK,
                json!({"object":"response.compaction","output":[]}),
            )
        }),
    ))
    .await;
    let original = route(&upstream.base, "route-a", PROFILE_A);
    let mut service = gateway(vec![original.clone()]).await;
    let response = client()
        .post(url(&service, "/v1/responses/compact"))
        .bearer_auth(TOKEN)
        .json(&json!({"model":"route-a","input":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut moved = original.clone();
    moved.profile_id = PROFILE_B.into();
    assert!(service
        .update_catalog(GatewayCatalog {
            entries: vec![moved]
        })
        .is_err());
    let mut versioned = original.clone();
    versioned.base_url.push_str("/v1");
    service
        .update_catalog(GatewayCatalog {
            entries: vec![versioned],
        })
        .unwrap();
    let mut other_host = original.clone();
    other_host.base_url = "https://unrelated.example/v1".into();
    assert!(service
        .update_catalog(GatewayCatalog {
            entries: vec![other_host]
        })
        .is_err());
    service
        .update_catalog(GatewayCatalog { entries: vec![] })
        .unwrap();
    assert_eq!(
        post(&service, "route-a", false)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    service.shutdown().await;
}

#[tokio::test]
async fn sticky_turn_state_is_only_sent_to_the_channel_that_issued_it() {
    async fn handler(headers: HeaderMap) -> Response<Body> {
        let mut response = json_response(
            StatusCode::OK,
            json!({"object":"response","model":"gpt-test","received_state":headers.get("x-codex-turn-state").and_then(|v|v.to_str().ok())}),
        );
        response
            .headers_mut()
            .insert("x-codex-turn-state", HeaderValue::from_static("sticky-a"));
        response
            .headers_mut()
            .insert("x-request-id", HeaderValue::from_static(KEY));
        response
    }
    let upstream = mock(Router::new().route("/responses", any(handler))).await;
    let mut service = gateway(vec![
        route(&upstream.base, "route-a", PROFILE_A),
        route(&upstream.base, "route-b", PROFILE_B),
    ])
    .await;
    let first = post(&service, "route-a", false).send().await.unwrap();
    assert_eq!(first.headers()["x-codex-turn-state"], "sticky-a");
    assert!(!first.headers().contains_key("x-request-id"));
    let same: Value = post(&service, "route-a", false)
        .header("x-codex-turn-state", "sticky-a")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(same["received_state"], "sticky-a");
    let other: Value = post(&service, "route-b", false)
        .header("x-codex-turn-state", "sticky-a")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(other["received_state"].is_null());
    service.shutdown().await;
}

#[tokio::test]
async fn stream_handles_split_utf8_and_a_truncated_final_event_is_an_error() {
    let upstream = mock(Router::new().route("/responses", any(|| async {
        let data = format!("data: {{\"type\":\"response.output_text.delta\",\"delta\":\"中文 {KEY}\"}}\n\ndata: {{\"type\":\"truncated\"");
        let chunks: Vec<_> = data.as_bytes().chunks(3).map(|chunk| Ok::<Bytes, Infallible>(Bytes::copy_from_slice(chunk))).collect();
        Response::builder().header(header::CONTENT_TYPE, "text/event-stream").body(Body::from_stream(stream::iter(chunks))).unwrap()
    }))).await;
    let mut service = gateway(vec![route(&upstream.base, "route-a", PROFILE_A)]).await;
    let response = post(&service, "route-a", true)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(response.contains(&format!("中文 {KEY}")));
    assert!(response.contains("ahax_upstream_stream_interrupted"));
    service.shutdown().await;
}

#[tokio::test]
async fn ordinary_json_text_and_tool_arguments_are_not_rewritten_when_key_is_a_common_word() {
    let original = json!({
        "object": "response", "id": "resp_plain", "model": "gpt-test",
        "output": [
            {"type":"message","content":[{"type":"output_text","text":"Run tests in the local folder; a test is ordinary text. 中文"}]},
            {"type":"function_call","name":"run_tests","arguments":"{\"command\":\"npm test\",\"testCase\":\"local\"}"}
        ],
        "usage":{"input_tokens":123,"output_tokens":45},
        "metadata":{"test":"preserve this key and value"}
    });
    let sent = original.clone();
    let upstream = mock(Router::new().route(
        "/responses",
        any(move || {
            let value = sent.clone();
            async move { json_response(StatusCode::OK, value) }
        }),
    ))
    .await;
    let mut service = start_with_secrets(
        GatewayCatalog {
            entries: vec![route(&upstream.base, "route-a", PROFILE_A)],
        },
        TOKEN.into(),
        0,
        Arc::new(|_| Ok("test".into())),
    )
    .await
    .unwrap();
    let mut expected = original;
    expected["model"] = json!("route-a");
    let response: Value = post(&service, "route-a", false)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response, expected);
    service.shutdown().await;
}

#[tokio::test]
async fn multiline_sse_preserves_success_text_but_redacts_failed_response_error() {
    let upstream = mock(Router::new().route("/responses",any(||async {
        let frames = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\ndata: \"delta\":\"run tests\"}\n\nevent: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_error\",\"model\":\"gpt-test\",\"error\":{\"message\":\"bad credential test\"}}}\n\n";
        Response::builder().header(header::CONTENT_TYPE,"text/event-stream").body(Body::from(frames)).unwrap()
    }))).await;
    let mut service = start_with_secrets(
        GatewayCatalog {
            entries: vec![route(&upstream.base, "route-a", PROFILE_A)],
        },
        TOKEN.into(),
        0,
        Arc::new(|_| Ok("test".into())),
    )
    .await
    .unwrap();
    let response = post(&service, "route-a", true)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(response.contains("run tests"));
    assert!(response.contains("bad credential [redacted]"));
    assert!(!response.contains("bad credential test"));
    assert!(response.contains("route-a"));
    service.shutdown().await;
}

#[test]
fn continuation_collision_and_capacity_never_cross_route_boundaries() {
    let a = route("https://a.example/v1", "a", PROFILE_A);
    let b = route("https://b.example/v1", "b", PROFILE_B);
    let mut ids = Continuations::default();
    ids.remember("resp_collision", &a);
    ids.remember("resp_collision", &b);
    assert!(!ids.matches("resp_collision", &a));
    assert!(!ids.matches("resp_collision", &b));
    for i in 0..MAX_CONTINUATIONS + 10 {
        ids.remember(&format!("resp_{i}"), &a);
    }
    assert!(ids.entries.len() <= MAX_CONTINUATIONS);
    assert!(!ids.matches("resp_0", &a));
    assert!(ids.matches(&format!("resp_{}", MAX_CONTINUATIONS + 9), &a));
}

#[tokio::test]
async fn legacy_and_current_aliases_use_the_real_upstream_model_and_share_continuations() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let upstream = mock(Router::new().fallback(any(move |body: Bytes| {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            let payload: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(payload["model"], "gpt-test");
            json_response(
                StatusCode::OK,
                json!({"id":"resp_legacy", "object":"response", "model":"gpt-test"}),
            )
        }
    })))
    .await;
    let current = format!("ahax-{}", "a".repeat(64));
    let legacy = format!("vela-{}", "a".repeat(64));
    let mut service = gateway(vec![route(&upstream.base, &current, PROFILE_A)]).await;
    for alias in [&legacy, &current] {
        let response = post(&service, alias, false).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<Value>().await.unwrap()["model"],
            alias.as_str()
        );
    }
    for alias in [&legacy, &current] {
        let response = client()
            .post(url(&service, "/v1/responses/compact"))
            .bearer_auth(TOKEN)
            .json(&json!({"model":alias,"previous_response_id":"resp_legacy","input":[]}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<Value>().await.unwrap()["model"],
            alias.as_str()
        );
    }
    assert_eq!(hits.load(Ordering::SeqCst), 4);
    let response = post(&service, &format!("vela-{}", "b".repeat(64)), false)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(hits.load(Ordering::SeqCst), 4);
    service.shutdown().await;
}

#[test]
fn internal_aliases_cannot_be_used_as_upstream_models_or_duplicate_catalog_entries() {
    let current = format!("ahax-{}", "a".repeat(64));
    let legacy = format!("vela-{}", "a".repeat(64));
    for alias in [&current, &legacy] {
        let mut invalid = route("https://example.test/v1", "route-a", PROFILE_A);
        invalid.upstream_model = alias.clone();
        assert!(validate_catalog(&GatewayCatalog {
            entries: vec![invalid]
        })
        .is_err());
    }
    assert!(validate_catalog(&GatewayCatalog {
        entries: vec![
            route("https://example.test/v1", &current, PROFILE_A),
            route("https://example.test/v1", &legacy, PROFILE_B),
        ]
    })
    .is_err());
}

#[tokio::test]
async fn corrupted_runtime_catalog_is_blocked_before_credentials_or_upstream_are_used() {
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
    let mut service = start_with_secrets(
        GatewayCatalog {
            entries: vec![route("https://example.test/v1", "route-a", PROFILE_A)],
        },
        TOKEN.into(),
        0,
        Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(KEY.into())
        }),
    )
    .await
    .unwrap();
    service.state.catalog.write().unwrap().entries[0].upstream_model =
        format!("vela-{}", "c".repeat(64));
    let response = post(&service, "route-a", false).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "internal_model_cannot_be_forwarded"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    service.shutdown().await;
}

#[tokio::test]
async fn renamed_catalog_entries_cannot_reassign_a_legacy_route_to_another_channel() {
    let current = format!("ahax-{}", "a".repeat(64));
    let legacy = format!("vela-{}", "a".repeat(64));
    let original = route("https://example.test/v1", &legacy, PROFILE_A);
    let mut service = gateway(vec![original.clone()]).await;
    let mut renamed = original;
    renamed.route_id = current;
    service
        .update_catalog(GatewayCatalog {
            entries: vec![renamed.clone()],
        })
        .unwrap();
    renamed.route_id = legacy;
    renamed.profile_id = PROFILE_B.into();
    assert!(service
        .update_catalog(GatewayCatalog {
            entries: vec![renamed]
        })
        .is_err());
    service.shutdown().await;
}
