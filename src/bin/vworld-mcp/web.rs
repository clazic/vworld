//! 단독 Streamable HTTP 모드(`vworld-mcp --http [주소]`, 경로 `/mcp`).
//!
//! 세션·SSE·JSON-RPC 처리는 rmcp `StreamableHttpService`(세션 모드)에 맡기고, 이 모듈은 그 앞의
//! 문지기다(규약 2단계 A):
//! - 인증(`VWORLD_MCP_TOKEN` Bearer, 상수 시간 비교), loopback 이 아닌 주소는 토큰 없으면 시작 거부.
//! - `Origin` 검사(loopback 출처 + `VWORLD_MCP_ALLOWED_ORIGINS`), 본문 1MB · 배치 10개 상한.
//! - 세션 id 는 바깥에 256bit 무작위 값을 따로 내주고 rmcp 세션 id 와 짝지어 둔다(1시간 미사용 시 버림).
//! - `MCP-Protocol-Version` 이 모르는 값이면 400.
//! - `X-Api-Keys` 머리글 → 그 요청의 tools/call `_meta["kr.pes/keys"]` 로 넣는다.
//! - 응답: 진행 알림이 먼저 나오면 SSE(알림들 → 응답), 아니면 application/json. 모든 데이터는 키 스크럽.
//! - SSE 연결이 응답 전에 끊기면 그 요청을 취소(`notifications/cancelled`)로 돌린다.
//! - 접근 기록(stderr)은 메서드 · 도구 이름 · 걸린 시간만.

use crate::{cancel_key, take_cancelled, Server};
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode};
use http_body_util::{combinators::BoxBody, BodyExt, Full, Limited};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{SocketAddr, ToSocketAddrs};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tower_service::Service;
use vworld::mcp;
use vworld::mcp_results::Store;

type Body = BoxBody<Bytes, Infallible>;

/// 요청 본문 상한.
pub const MAX_BODY: usize = 1024 * 1024;
/// 배치 상한.
pub const MAX_BATCH: usize = 10;
/// 세션 미사용 만료.
pub const SESSION_IDLE: Duration = Duration::from_secs(3600);
/// 받아 주는 `MCP-Protocol-Version`.
pub const KNOWN_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
const SESSION_HEADER: &str = "mcp-session-id";

/// HTTP 모드 설정.
#[derive(Debug, Clone)]
pub struct Options {
    pub addr: String,
    pub token: Option<String>,
    pub allowed_origins: Vec<String>,
}

impl Options {
    /// 환경변수 `VWORLD_MCP_TOKEN` · `VWORLD_MCP_ALLOWED_ORIGINS` 를 읽는다.
    pub fn from_env(addr: &str) -> Self {
        let token = std::env::var("VWORLD_MCP_TOKEN")
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let allowed_origins = std::env::var("VWORLD_MCP_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(|o| o.trim().trim_end_matches('/').to_string())
            .filter(|o| !o.is_empty())
            .collect();
        Options {
            addr: addr.to_string(),
            token,
            allowed_origins,
        }
    }
}

/// 주소를 풀고, loopback 이 아닌데 토큰이 없으면 거부한다.
pub fn resolve_and_check(opts: &Options) -> Result<SocketAddr, String> {
    let addrs: Vec<SocketAddr> = opts
        .addr
        .to_socket_addrs()
        .map_err(|e| format!("주소를 풀 수 없어요({}): {e}", opts.addr))?
        .collect();
    let first = *addrs
        .first()
        .ok_or_else(|| format!("주소를 풀 수 없어요: {}", opts.addr))?;
    let loopback = addrs.iter().all(|a| a.ip().is_loopback());
    if !loopback && opts.token.is_none() {
        return Err(format!(
            "{} 은 loopback 주소가 아니에요. VWORLD_MCP_TOKEN 없이는 시작하지 않아요",
            opts.addr
        ));
    }
    Ok(first)
}

/// 상수 시간 비교(길이가 다르면 바로 false).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Origin 허용 여부: loopback 출처(`http://localhost|127.0.0.1|[::1]` + 선택 포트) 또는 허용 목록.
pub fn origin_allowed(origin: &str, allowed: &[String]) -> bool {
    let o = origin.trim_end_matches('/');
    if allowed.iter().any(|a| a == o) {
        return true;
    }
    ["http://localhost", "http://127.0.0.1", "http://[::1]"]
        .iter()
        .any(|base| match o.strip_prefix(base) {
            Some("") => true,
            Some(rest) => rest.strip_prefix(':').is_some_and(|p| {
                !p.is_empty() && p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit())
            }),
            None => false,
        })
}

fn text(status: StatusCode, msg: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(msg.to_string())).boxed())
        .expect("valid response")
}

fn json_resp(status: StatusCode, v: &Value, secrets: &[String]) -> Response<Body> {
    let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
    let body = mcp::scrub(&v.to_string(), &refs);
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)).boxed())
        .expect("valid response")
}

fn rpc_error(status: StatusCode, code: i64, msg: &str) -> Response<Body> {
    json_resp(
        status,
        &json!({"jsonrpc": "2.0", "id": Value::Null, "error": {"code": code, "message": msg}}),
        &[],
    )
}

fn new_session_id() -> String {
    // uuid v4 두 개(무작위 244bit) — 128bit 이상.
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// 바깥 세션 id → (rmcp 세션 id, 마지막 사용).
#[derive(Default)]
struct Sessions(Mutex<HashMap<String, (String, Instant)>>);

impl Sessions {
    fn insert(&self, internal: String) -> String {
        let ext = new_session_id();
        self.0
            .lock()
            .unwrap()
            .insert(ext.clone(), (internal, Instant::now()));
        ext
    }
    /// 바깥 id 를 풀고 사용 시각을 갱신. 만료된 것은 이참에 버린다.
    fn resolve(&self, ext: &str) -> Option<String> {
        let mut m = self.0.lock().unwrap();
        m.retain(|_, (_, t)| t.elapsed() < SESSION_IDLE);
        m.get_mut(ext).map(|(int, t)| {
            *t = Instant::now();
            int.clone()
        })
    }
    fn remove(&self, ext: &str) {
        self.0.lock().unwrap().remove(ext);
    }
}

/// HTTP 문지기.
pub struct Gateway {
    svc: StreamableHttpService<Server, LocalSessionManager>,
    token: Option<String>,
    origins: Vec<String>,
    defaults: Vec<String>,
    sessions: Sessions,
    store: Store,
}

impl Gateway {
    pub fn new(server: Server, opts: &Options, loopback: bool) -> Arc<Self> {
        let mut manager = LocalSessionManager::default();
        manager.session_config.keep_alive = Some(SESSION_IDLE);
        manager.session_config.sse_retry = None;
        let mut config = StreamableHttpServerConfig::default()
            .with_sse_keep_alive(Some(Duration::from_secs(15)))
            .with_sse_retry(None)
            .with_max_request_body_bytes(MAX_BODY);
        // 문지기가 Host 대신 Origin·토큰으로 막는다. 내부 전달 요청은 Host: localhost.
        if !loopback {
            config = config.disable_allowed_hosts();
        }
        let defaults = server.default_secrets();
        let store = server.store();
        let factory = move || Ok(server.clone());
        Arc::new(Gateway {
            svc: StreamableHttpService::new(factory, Arc::new(manager), config),
            token: opts.token.clone(),
            origins: opts.allowed_origins.clone(),
            defaults,
            sessions: Sessions::default(),
            store,
        })
    }

    /// `GET /results/<id>.<csv|geojson>` — 저장된 결과 파일을 흘려 보낸다.
    async fn download(&self, file: &str) -> Response<Body> {
        let Some((path, mime)) = self.store.download_path(file) else {
            return text(
                StatusCode::NOT_FOUND,
                "Not Found: 결과가 없거나 만료됐어요 — 다시 조회하세요",
            );
        };
        let Ok(mut f) = tokio::fs::File::open(&path).await else {
            return text(StatusCode::NOT_FOUND, "Not Found: 결과가 없거나 만료됐어요");
        };
        let len = f.metadata().await.map(|m| m.len()).unwrap_or(0);
        let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(8);
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match f.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(Bytes::copy_from_slice(&buf[..n])).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let ctype = if mime == "text/csv" {
            "text/csv; charset=utf-8"
        } else {
            mime
        };
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", ctype)
            .header("content-length", len)
            .header(
                "content-disposition",
                format!("attachment; filename=\"{file}\""),
            )
            .body(BodyExt::boxed(ChannelBody(rx)))
            .expect("valid response")
    }

    async fn call_inner(&self, req: Request<Full<Bytes>>) -> Response<Body> {
        let mut svc = self.svc.clone();
        match svc.call(req).await {
            Ok(r) => r,
            Err(never) => match never {},
        }
    }

    /// rmcp 로 넘길 요청(머리글은 꼭 필요한 것만).
    fn inner_request(
        method: Method,
        body: Bytes,
        headers: &HeaderMap,
        internal_session: Option<&str>,
    ) -> Request<Full<Bytes>> {
        let mut b = Request::builder()
            .method(method)
            .uri("http://localhost/mcp")
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(v) = headers.get("mcp-protocol-version") {
            b = b.header("mcp-protocol-version", v);
        }
        if let Some(s) = internal_session {
            b = b.header(SESSION_HEADER, s);
        }
        b.body(Full::new(body)).expect("valid request")
    }

    /// 요청 하나를 처리한다(접근 기록 포함).
    pub async fn handle<B>(self: Arc<Self>, req: Request<B>) -> Response<Body>
    where
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        let started = Instant::now();
        let method = req.method().clone();
        let (resp, label) = self.route(req).await;
        eprintln!(
            "[http] {method} {label} → {} {}ms",
            resp.status().as_u16(),
            started.elapsed().as_millis()
        );
        resp
    }

    async fn route<B>(&self, req: Request<B>) -> (Response<Body>, String)
    where
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        if let Some(o) = req.headers().get("origin") {
            let ok = o.to_str().is_ok_and(|o| origin_allowed(o, &self.origins));
            if !ok {
                return (
                    text(StatusCode::FORBIDDEN, "Forbidden: 허용되지 않은 Origin"),
                    "-".into(),
                );
            }
        }
        if let Some(t) = &self.token {
            let expected = format!("Bearer {t}");
            let got = req
                .headers()
                .get("authorization")
                .map(|v| v.as_bytes())
                .unwrap_or(b"");
            if !ct_eq(got, expected.as_bytes()) {
                let mut r = text(
                    StatusCode::UNAUTHORIZED,
                    "Unauthorized: Bearer 토큰이 필요해요",
                );
                r.headers_mut()
                    .insert("www-authenticate", HeaderValue::from_static("Bearer"));
                return (r, "-".into());
            }
        }
        // 결과 파일 내려받기(같은 토큰·Origin 검사를 거친 뒤).
        if let Some(file) = req.uri().path().strip_prefix("/results/") {
            if req.method() != Method::GET {
                return (
                    text(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed"),
                    "-".into(),
                );
            }
            let file = file.to_string();
            return (self.download(&file).await, "GET results".into());
        }
        if req.uri().path() != "/mcp" {
            return (
                text(
                    StatusCode::NOT_FOUND,
                    "Not Found: /mcp · /results/<id>.<csv|geojson> 만 있어요",
                ),
                "-".into(),
            );
        }
        if let Some(v) = req.headers().get("mcp-protocol-version") {
            if !v.to_str().is_ok_and(|v| KNOWN_VERSIONS.contains(&v)) {
                return (
                    text(
                        StatusCode::BAD_REQUEST,
                        "Bad Request: 모르는 MCP-Protocol-Version",
                    ),
                    "-".into(),
                );
            }
        }
        match *req.method() {
            Method::POST => self.post(req).await,
            Method::GET => (
                self.get_or_delete(req, Method::GET).await,
                "GET-stream".into(),
            ),
            Method::DELETE => (
                self.get_or_delete(req, Method::DELETE).await,
                "DELETE".into(),
            ),
            _ => (
                text(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed"),
                "-".into(),
            ),
        }
    }

    /// 세션 머리글 → rmcp 세션 id. 없으면 400, 모르면 404.
    #[allow(clippy::result_large_err)]
    fn session(&self, headers: &HeaderMap) -> Result<(String, String), Response<Body>> {
        let Some(ext) = headers.get(SESSION_HEADER).and_then(|v| v.to_str().ok()) else {
            return Err(text(
                StatusCode::BAD_REQUEST,
                "Bad Request: Mcp-Session-Id 머리글이 필요해요",
            ));
        };
        match self.sessions.resolve(ext) {
            Some(int) => Ok((ext.to_string(), int)),
            None => Err(text(
                StatusCode::NOT_FOUND,
                "Not Found: 모르는 세션이에요(다시 initialize)",
            )),
        }
    }

    async fn get_or_delete<B>(&self, req: Request<B>, method: Method) -> Response<Body> {
        let (ext, int) = match self.session(req.headers()) {
            Ok(s) => s,
            Err(r) => return r,
        };
        if method == Method::GET {
            let accepts_sse = req
                .headers()
                .get("accept")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|a| a.contains("text/event-stream"));
            if !accepts_sse {
                return text(
                    StatusCode::NOT_ACCEPTABLE,
                    "Not Acceptable: Accept: text/event-stream 필요",
                );
            }
        }
        let inner = Self::inner_request(method.clone(), Bytes::new(), req.headers(), Some(&int));
        let r = self.call_inner(inner).await;
        if method == Method::DELETE {
            self.sessions.remove(&ext);
        }
        r
    }

    async fn post<B>(&self, req: Request<B>) -> (Response<Body>, String)
    where
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        let (parts, body) = req.into_parts();
        let bytes = match Limited::new(body, MAX_BODY).collect().await {
            Ok(c) => c.to_bytes(),
            Err(e) => {
                let too_big = e
                    .downcast_ref::<http_body_util::LengthLimitError>()
                    .is_some();
                return if too_big {
                    (
                        text(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "Payload Too Large: 본문은 1MB 이하",
                        ),
                        "-".into(),
                    )
                } else {
                    (
                        text(StatusCode::BAD_REQUEST, "Bad Request: 본문을 읽지 못했어요"),
                        "-".into(),
                    )
                };
            }
        };
        let msg: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                return (
                    rpc_error(StatusCode::BAD_REQUEST, -32700, "Parse error"),
                    "-".into(),
                )
            }
        };
        // X-Api-Keys 검사(형식은 기본 키와 같음). 값은 오류에 넣지 않는다.
        let api_keys: Vec<String> = match parts.headers.get("x-api-keys") {
            None => Vec::new(),
            Some(v) => {
                let raw = v.to_str().unwrap_or("\u{0}");
                let list: Vec<Value> = raw
                    .split(',')
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .map(|k| Value::String(k.to_string()))
                    .collect();
                match mcp::select_auths(Some(&Value::Array(list)), &[], None) {
                    Ok((a, _)) => a.into_iter().map(|a| a.key).collect(),
                    Err(e) => {
                        let e = e.replace("_meta[\"kr.pes/keys\"]", "X-Api-Keys");
                        return (
                            text(StatusCode::BAD_REQUEST, &format!("Bad Request: {e}")),
                            "-".into(),
                        );
                    }
                }
            }
        };
        let accepts_sse = parts
            .headers
            .get("accept")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|a| a.contains("text/event-stream"));

        match msg {
            Value::Array(items) => {
                if items.is_empty() || items.len() > MAX_BATCH {
                    return (
                        rpc_error(StatusCode::BAD_REQUEST, -32600, "배치는 1~10개예요"),
                        "batch".into(),
                    );
                }
                let mut out = Vec::new();
                let mut labels = Vec::new();
                for it in items {
                    labels.push(label_of(&it));
                    if it.get("method").and_then(Value::as_str) == Some("initialize") {
                        return (
                            rpc_error(
                                StatusCode::BAD_REQUEST,
                                -32600,
                                "initialize 는 배치에 넣을 수 없어요",
                            ),
                            "batch".into(),
                        );
                    }
                    match self.post_one(it, &parts.headers, false, &api_keys).await {
                        Ok(Outcome::Json(v, _)) => out.push(v),
                        Ok(Outcome::Accepted | Outcome::NoReply) => {}
                        Ok(Outcome::Stream(_)) => unreachable!("배치는 SSE 를 쓰지 않음"),
                        Err(r) => return (r, format!("batch[{}]", labels.join(","))),
                    }
                }
                let label = format!("batch[{}]", labels.join(","));
                if out.is_empty() {
                    return (text(StatusCode::ACCEPTED, ""), label);
                }
                let secrets = self.secrets(&api_keys);
                (
                    json_resp(StatusCode::OK, &Value::Array(out), &secrets),
                    label,
                )
            }
            single => {
                let label = label_of(&single);
                let r = match self
                    .post_one(single, &parts.headers, accepts_sse, &api_keys)
                    .await
                {
                    Ok(Outcome::Json(v, session)) => {
                        let mut r = json_resp(StatusCode::OK, &v, &self.secrets(&api_keys));
                        if let Some(s) = session {
                            r.headers_mut()
                                .insert(SESSION_HEADER, HeaderValue::from_str(&s).expect("hex"));
                        }
                        r
                    }
                    Ok(Outcome::Stream(r)) => r,
                    Ok(Outcome::Accepted) => text(StatusCode::ACCEPTED, ""),
                    Ok(Outcome::NoReply) => text(StatusCode::NO_CONTENT, ""),
                    Err(r) => r,
                };
                (r, label)
            }
        }
    }

    fn secrets(&self, extra: &[String]) -> Vec<String> {
        self.defaults.iter().chain(extra.iter()).cloned().collect()
    }

    /// JSON-RPC 메시지 하나를 rmcp 로 넘기고 결과를 고른다.
    #[allow(clippy::result_large_err)]
    async fn post_one(
        &self,
        mut msg: Value,
        headers: &HeaderMap,
        allow_sse: bool,
        api_keys: &[String],
    ) -> Result<Outcome, Response<Body>> {
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string);
        let id = msg.get("id").cloned().filter(|v| !v.is_null());

        if method.as_deref() == Some("initialize") && id.is_some() {
            let inner =
                Self::inner_request(Method::POST, Bytes::from(msg.to_string()), headers, None);
            let resp = self.call_inner(inner).await;
            if resp.status() != StatusCode::OK {
                return Err(resp);
            }
            let internal = resp
                .headers()
                .get(SESSION_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let mut reader = SseReader::new(resp);
            while let Some(ev) = reader.next().await {
                if let Some(v) = ev.data.and_then(|d| serde_json::from_str::<Value>(&d).ok()) {
                    if is_response(&v) {
                        let ext = internal.map(|i| self.sessions.insert(i));
                        return Ok(Outcome::Json(v, ext));
                    }
                }
            }
            return Err(text(
                StatusCode::INTERNAL_SERVER_ERROR,
                "initialize 응답이 없어요",
            ));
        }

        let (_ext, internal) = self.session(headers)?;
        let mut call_keys: Vec<String> = api_keys.to_vec();
        if method.as_deref() == Some("tools/call") {
            if let Some(params) = msg.get_mut("params").and_then(Value::as_object_mut) {
                let meta = params
                    .entry("_meta")
                    .or_insert_with(|| json!({}))
                    .as_object_mut();
                if let Some(meta) = meta {
                    if !api_keys.is_empty() && !meta.contains_key(mcp::META_KEYS) {
                        meta.insert(mcp::META_KEYS.into(), json!(api_keys));
                    }
                    if let Some(Value::Array(ks)) = meta.get(mcp::META_KEYS) {
                        call_keys.extend(ks.iter().filter_map(|k| k.as_str().map(str::to_string)));
                    }
                }
            }
        }
        let secrets = self.secrets(&call_keys);
        let inner = Self::inner_request(
            Method::POST,
            Bytes::from(msg.to_string()),
            headers,
            Some(&internal),
        );
        let resp = self.call_inner(inner).await;
        match resp.status() {
            StatusCode::ACCEPTED => return Ok(Outcome::Accepted),
            StatusCode::OK => {}
            _ => return Err(resp),
        }
        let is_sse = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|c| c.starts_with("text/event-stream"));
        if !is_sse {
            let body = resp
                .into_body()
                .collect()
                .await
                .map(|c| c.to_bytes())
                .unwrap_or_default();
            let v = serde_json::from_slice(&body).unwrap_or(Value::Null);
            return Ok(Outcome::Json(v, None));
        }
        let Some(id) = id else {
            return Ok(Outcome::Accepted);
        };
        let key = cancel_key(&internal, &id.to_string());
        let mut reader = SseReader::new(resp);
        loop {
            let Some(ev) = reader.next().await else {
                return Ok(Outcome::NoReply); // 응답 없이 끝남(취소 등)
            };
            let Some(v) = ev.data.and_then(|d| serde_json::from_str::<Value>(&d).ok()) else {
                continue; // 주석·빈 이벤트
            };
            if is_response(&v) {
                if take_cancelled(&key) {
                    return Ok(Outcome::NoReply);
                }
                return Ok(Outcome::Json(v, None));
            }
            if allow_sse {
                // 알림이 먼저 왔다 — SSE 로 바꿔 이 알림부터 흘려 보낸다.
                let first = sse_frame(&v, &secrets);
                return Ok(Outcome::Stream(
                    self.stream(first, reader, key, internal, id, secrets),
                ));
            }
            // JSON 만 받는 클라이언트(또는 배치): 알림은 버리고 응답을 기다린다.
        }
    }

    /// 남은 SSE 를 클라이언트로 흘려 보내는 응답. 클라이언트가 응답 전에 끊으면 그 요청을 취소한다.
    fn stream(
        &self,
        first: Bytes,
        mut reader: SseReader,
        key: String,
        internal: String,
        id: Value,
        secrets: Vec<String>,
    ) -> Response<Body> {
        let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(32);
        let svc = self.svc.clone();
        tokio::spawn(async move {
            let mut delivered = tx.send(first).await.is_ok();
            let mut answered = false;
            while delivered {
                let ev = tokio::select! {
                    ev = reader.next() => ev,
                    _ = tx.closed() => { delivered = false; break; }
                };
                let Some(ev) = ev else { break };
                let frame = match ev.data.and_then(|d| serde_json::from_str::<Value>(&d).ok()) {
                    Some(v) => {
                        if is_response(&v) {
                            answered = true;
                            if take_cancelled(&key) {
                                break;
                            }
                            let f = sse_frame(&v, &secrets);
                            let _ = tx.send(f).await;
                            break;
                        }
                        sse_frame(&v, &secrets)
                    }
                    None if ev.comment => Bytes::from_static(b": ping\n\n"),
                    None => continue,
                };
                delivered = tx.send(frame).await.is_ok();
            }
            if !delivered && !answered {
                // 클라이언트가 끊었다 — 그 요청을 취소로 돌린다.
                let note = json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
                    "params": {"requestId": id, "reason": "HTTP 연결 끊김"}});
                let req = Gateway::inner_request(
                    Method::POST,
                    Bytes::from(note.to_string()),
                    &HeaderMap::new(),
                    Some(&internal),
                );
                let mut svc = svc;
                let _ = svc.call(req).await;
            }
        });
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .body(BodyExt::boxed(ChannelBody(rx)))
            .expect("valid response")
    }
}

enum Outcome {
    /// JSON 응답(+ initialize 면 바깥 세션 id).
    Json(Value, Option<String>),
    /// SSE 로 흘려 보내는 중.
    Stream(Response<Body>),
    /// 알림 · 응답 — 202.
    Accepted,
    /// 응답 없음(취소된 요청).
    NoReply,
}

fn is_response(v: &Value) -> bool {
    v.get("method").is_none() && (v.get("result").is_some() || v.get("error").is_some())
}

/// 접근 기록용 이름: 메서드(+ 도구 이름). 인자·키는 넣지 않는다.
fn label_of(v: &Value) -> String {
    let m = v
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("response");
    match v.pointer("/params/name").and_then(Value::as_str) {
        Some(tool) if m == "tools/call" => format!("{m} {tool}"),
        _ => m.to_string(),
    }
}

fn sse_frame(v: &Value, secrets: &[String]) -> Bytes {
    let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
    Bytes::from(format!(
        "event: message\ndata: {}\n\n",
        mcp::scrub(&v.to_string(), &refs)
    ))
}

/// SSE 이벤트 하나(데이터 줄을 이은 것 · 주석 여부).
struct SseEvent {
    data: Option<String>,
    comment: bool,
}

/// rmcp SSE 응답 본문을 이벤트 단위로 읽는다.
struct SseReader {
    body: Body,
    buf: Vec<u8>,
    done: bool,
}

impl SseReader {
    fn new(resp: Response<Body>) -> Self {
        SseReader {
            body: resp.into_body(),
            buf: Vec::new(),
            done: false,
        }
    }

    async fn next(&mut self) -> Option<SseEvent> {
        loop {
            if let Some(pos) = find_block_end(&self.buf) {
                let block: Vec<u8> = self.buf.drain(..pos.0).collect();
                self.buf.drain(..pos.1);
                return Some(parse_block(&block));
            }
            if self.done {
                if self.buf.is_empty() {
                    return None;
                }
                let block = std::mem::take(&mut self.buf);
                return Some(parse_block(&block));
            }
            match self.body.frame().await {
                Some(Ok(f)) => {
                    if let Ok(d) = f.into_data() {
                        self.buf.extend_from_slice(&d);
                    }
                }
                Some(Err(never)) => match never {},
                None => self.done = true,
            }
        }
    }
}

/// 빈 줄(이벤트 끝) 위치: (블록 길이, 구분자 길이).
fn find_block_end(buf: &[u8]) -> Option<(usize, usize)> {
    for i in 0..buf.len() {
        if buf[i..].starts_with(b"\n\n") {
            return Some((i, 2));
        }
        if buf[i..].starts_with(b"\r\n\r\n") {
            return Some((i, 4));
        }
    }
    None
}

fn parse_block(block: &[u8]) -> SseEvent {
    let s = String::from_utf8_lossy(block);
    let mut data: Vec<&str> = Vec::new();
    let mut comment = false;
    for line in s.lines() {
        if let Some(d) = line.strip_prefix("data:") {
            data.push(d.strip_prefix(' ').unwrap_or(d));
        } else if line.starts_with(':') {
            comment = true;
        }
    }
    let data = (!data.is_empty())
        .then(|| data.join("\n"))
        .filter(|d| !d.is_empty());
    SseEvent { data, comment }
}

/// mpsc 수신기를 HTTP 본문으로.
struct ChannelBody(tokio::sync::mpsc::Receiver<Bytes>);

impl http_body::Body for ChannelBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Infallible>>> {
        self.0
            .poll_recv(cx)
            .map(|o| o.map(|b| Ok(http_body::Frame::data(b))))
    }
}

/// HTTP 서버 실행(끝나지 않음).
pub async fn serve(server: Server, opts: Options) -> Result<(), String> {
    let addr = resolve_and_check(&opts)?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("{addr} 에 열 수 없어요: {e}"))?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    let gw = Gateway::new(server, &opts, local.ip().is_loopback());
    eprintln!(
        "vworld-mcp HTTP — http://{local}/mcp (토큰 {}, 허용 Origin 추가 {}개)",
        if opts.token.is_some() {
            "필수"
        } else {
            "없음"
        },
        opts.allowed_origins.len()
    );
    run_listener(listener, gw).await
}

/// 이미 열린 리스너로 요청을 받는다(시험에서도 씀).
pub async fn run_listener(
    listener: tokio::net::TcpListener,
    gw: Arc<Gateway>,
) -> Result<(), String> {
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[http] accept 실패: {e}");
                continue;
            }
        };
        let gw = gw.clone();
        tokio::spawn(async move {
            let svc = hyper::service::service_fn(move |req| {
                let gw = gw.clone();
                async move { Ok::<_, Infallible>(gw.handle(req).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vworld::api::Auth;

    #[test]
    fn origin_rules() {
        let allowed = vec!["https://pes.example.kr".to_string()];
        assert!(origin_allowed("http://localhost:3000", &allowed));
        assert!(origin_allowed("http://127.0.0.1", &allowed));
        assert!(origin_allowed("http://[::1]:8080", &allowed));
        assert!(origin_allowed("https://pes.example.kr", &allowed));
        assert!(!origin_allowed("http://evil.example", &allowed));
        assert!(!origin_allowed("http://localhost.evil.example", &allowed));
        assert!(!origin_allowed("http://localhost:3000@evil", &allowed));
        assert!(!origin_allowed("https://localhost:3000", &[]));
    }

    #[test]
    fn non_loopback_without_token_refused() {
        let mut o = Options {
            addr: "0.0.0.0:0".into(),
            token: None,
            allowed_origins: vec![],
        };
        assert!(resolve_and_check(&o)
            .unwrap_err()
            .contains("VWORLD_MCP_TOKEN"));
        o.token = Some("t".into());
        assert!(resolve_and_check(&o).is_ok());
        let local = Options {
            addr: "127.0.0.1:0".into(),
            token: None,
            allowed_origins: vec![],
        };
        assert!(resolve_and_check(&local).is_ok());
    }

    #[test]
    fn constant_time_eq() {
        assert!(ct_eq(b"Bearer abc", b"Bearer abc"));
        assert!(!ct_eq(b"Bearer abd", b"Bearer abc"));
        assert!(!ct_eq(b"Bearer ab", b"Bearer abc"));
    }

    #[test]
    fn session_ids_are_long_and_random() {
        let a = new_session_id();
        assert_eq!(a.len(), 64);
        assert_ne!(a, new_session_id());
    }

    // ── 실제 HTTP 서버를 띄워 보는 시험 ──

    const DEF: &str = "DEFAULTKEY-0000-1111-2222-3333AAAA";
    const HDR: &str = "HEADERKEY0-4444-5555-6666-7777BBBB";

    fn test_store() -> Store {
        let dir =
            std::env::temp_dir().join(format!("vworld-mcp-web-{}", vworld::mcp_results::new_id()));
        Store::new(dir, Duration::from_secs(3600), 1 << 30)
    }

    async fn start(token: Option<&str>, defaults: Vec<&str>) -> String {
        start_with(token, defaults, test_store()).await
    }

    async fn start_with(token: Option<&str>, defaults: Vec<&str>, store: Store) -> String {
        let defaults = defaults
            .into_iter()
            .map(|k| Auth {
                key: k.into(),
                domain: None,
            })
            .collect();
        let server = Server::new(defaults, None)
            .unwrap()
            .with_test_tools()
            .with_store(store);
        let opts = Options {
            addr: "127.0.0.1:0".into(),
            token: token.map(str::to_string),
            allowed_origins: vec![],
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let gw = Gateway::new(server, &opts, true);
        tokio::spawn(run_listener(listener, gw));
        url
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    fn post(c: &reqwest::Client, url: &str, body: Value) -> reqwest::RequestBuilder {
        c.post(url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(body.to_string())
    }

    async fn init(c: &reqwest::Client, url: &str, token: Option<&str>) -> String {
        let mut rb = post(
            c,
            url,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}),
        );
        if let Some(t) = token {
            rb = rb.bearer_auth(t);
        }
        let r = rb.send().await.unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json"));
        let sid = r.headers()[SESSION_HEADER].to_str().unwrap().to_string();
        let v: Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
        let mut rb = post(
            c,
            url,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .header(SESSION_HEADER, &sid);
        if let Some(t) = token {
            rb = rb.bearer_auth(t);
        }
        assert_eq!(rb.send().await.unwrap().status(), 202);
        sid
    }

    async fn call(
        c: &reqwest::Client,
        url: &str,
        sid: &str,
        id: u64,
        name: &str,
        args: Value,
    ) -> reqwest::Response {
        post(
            c,
            url,
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}),
        )
        .header(SESSION_HEADER, sid)
        .send()
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn http_session_flow_and_missing_key_error() {
        let url = start(None, vec![]).await;
        let c = client();
        let sid = init(&c, &url, None).await;
        assert_eq!(sid.len(), 64);
        let r = post(
            &c,
            &url,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .header(SESSION_HEADER, &sid)
        .send()
        .await
        .unwrap();
        assert_eq!(r.status(), 200);
        let v: Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
        assert!(v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "vworld_zoning"));
        let r = call(
            &c,
            &url,
            &sid,
            3,
            "vworld_geocode",
            json!({"address":"세종대로 110"}),
        )
        .await;
        let v: Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true);
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("키가 없어요"));
        // 배치(2개): ping + tools/list.
        let r = post(
            &c,
            &url,
            json!([{"jsonrpc":"2.0","id":10,"method":"ping"},{"jsonrpc":"2.0","id":11,"method":"tools/list"}]),
        )
        .header(SESSION_HEADER, &sid)
        .send()
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);
        // 배치 11개는 거부.
        let many: Vec<Value> = (0..11)
            .map(|i| json!({"jsonrpc":"2.0","id":100+i,"method":"ping"}))
            .collect();
        let r = post(&c, &url, Value::Array(many))
            .header(SESSION_HEADER, &sid)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        // 모르는 프로토콜 버전 400.
        let r = post(&c, &url, json!({"jsonrpc":"2.0","id":12,"method":"ping"}))
            .header(SESSION_HEADER, &sid)
            .header("mcp-protocol-version", "1999-01-01")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        // 세션 끝 → 이후 404.
        let r = c
            .delete(&url)
            .header(SESSION_HEADER, &sid)
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "{}", r.status());
        let r = post(&c, &url, json!({"jsonrpc":"2.0","id":13,"method":"ping"}))
            .header(SESSION_HEADER, &sid)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404);
    }

    #[tokio::test]
    async fn http_session_required_and_unknown() {
        let url = start(None, vec![]).await;
        let c = client();
        let r = post(
            &c,
            &url,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .send()
        .await
        .unwrap();
        assert_eq!(r.status(), 400);
        let r = post(
            &c,
            &url,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .header(SESSION_HEADER, "0".repeat(64))
        .send()
        .await
        .unwrap();
        assert_eq!(r.status(), 404);
        let r = c
            .get(&url)
            .header("accept", "text/event-stream")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        // 본문 1MB 초과 413.
        let big = "x".repeat(MAX_BODY + 10);
        let r = c
            .post(&url)
            .header("content-type", "application/json")
            .body(big)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 413);
    }

    #[tokio::test]
    async fn http_token_and_origin() {
        let url = start(Some("s3cret-token"), vec![]).await;
        let c = client();
        let body = json!({"jsonrpc":"2.0","id":1,"method":"ping"});
        let r = post(&c, &url, body.clone()).send().await.unwrap();
        assert_eq!(r.status(), 401);
        let r = post(&c, &url, body.clone())
            .bearer_auth("wrong")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
        let sid = init(&c, &url, Some("s3cret-token")).await;
        let r = post(&c, &url, body.clone())
            .bearer_auth("s3cret-token")
            .header(SESSION_HEADER, &sid)
            .header("origin", "http://evil.example")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
        let r = post(&c, &url, body)
            .bearer_auth("s3cret-token")
            .header(SESSION_HEADER, &sid)
            .header("origin", "http://localhost:5173")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
    }

    #[tokio::test]
    async fn http_x_api_keys_beat_default_key() {
        let url = start(None, vec![DEF]).await;
        let c = client();
        let sid = init(&c, &url, None).await;
        let r = post(
            &c,
            &url,
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"test_keys","arguments":{}}}),
        )
        .header(SESSION_HEADER, &sid)
        .header("x-api-keys", HDR)
        .send()
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
        let t = v["result"]["content"][0]["text"].as_str().unwrap();
        // 머리글 키만 쓰였고(기본 키 아님), 응답에는 원문 없이 ***끝4자리.
        assert!(t.contains("***BBBB") && !t.contains("***AAAA"), "{t}");
        assert!(!t.contains(HDR) && !t.contains(DEF), "{t}");
        // 머리글 형식이 틀리면 400.
        let r = post(&c, &url, json!({"jsonrpc":"2.0","id":6,"method":"ping"}))
            .header(SESSION_HEADER, &sid)
            .header("x-api-keys", "bad key!")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
    }

    /// SSE 본문을 이벤트 데이터 목록으로.
    fn sse_data(body: &str) -> Vec<Value> {
        body.split("\n\n")
            .filter_map(|b| b.lines().find_map(|l| l.strip_prefix("data: ")))
            .filter_map(|d| serde_json::from_str(d).ok())
            .collect()
    }

    #[tokio::test]
    async fn http_progress_arrives_before_result() {
        let url = start(None, vec![]).await;
        let c = client();
        let sid = init(&c, &url, None).await;
        let r = post(
            &c,
            &url,
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{
                "name":"vworld_zoning","arguments":{"x":500.0,"y":500.0},
                "_meta":{"progressToken":"p-7"}}}),
        )
        .header(SESSION_HEADER, &sid)
        .header("x-api-keys", HDR)
        .send()
        .await
        .unwrap();
        assert!(r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"));
        let events = sse_data(&r.text().await.unwrap());
        assert!(events.len() >= 2, "{events:?}");
        assert_eq!(events[0]["method"], "notifications/progress");
        assert_eq!(events[0]["params"]["progressToken"], "p-7");
        let last = events.last().unwrap();
        assert_eq!(last["id"], 7);
        assert_eq!(last["result"]["isError"], true); // 범위 밖 좌표 — 진행 시작 뒤 검증 오류.
                                                     // progressToken 이 없으면 알림 없이 JSON.
        let r = call(
            &c,
            &url,
            &sid,
            8,
            "vworld_zoning",
            json!({"x":500.0,"y":500.0}),
        )
        .await;
        assert!(r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json"));
    }

    #[tokio::test]
    async fn http_cancelled_stops_work_and_gets_no_reply() {
        let url = start(None, vec![]).await;
        let c = client();
        let sid = init(&c, &url, None).await;
        let tag = "cancel-note";
        let resp = post(
            &c,
            &url,
            json!({"jsonrpc":"2.0","id":21,"method":"tools/call","params":{
                "name":"test_sleep","arguments":{"tag":tag},"_meta":{"progressToken":1}}}),
        )
        .header(SESSION_HEADER, &sid)
        .send()
        .await
        .unwrap();
        assert!(resp.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"));
        let reader = tokio::spawn(async move { resp.text().await.unwrap_or_default() });
        tokio::time::sleep(Duration::from_millis(200)).await;
        let r = post(
            &c,
            &url,
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":21,"reason":"test"}}),
        )
        .header(SESSION_HEADER, &sid)
        .send()
        .await
        .unwrap();
        assert_eq!(r.status(), 202);
        let body = tokio::time::timeout(Duration::from_secs(10), reader)
            .await
            .unwrap()
            .unwrap();
        let events = sse_data(&body);
        assert!(
            events.iter().all(|e| e.get("id").is_none()),
            "취소된 요청에 응답이 왔어요: {events:?}"
        );
        assert!(crate::test_tools::aborted(tag), "작업이 멈추지 않았어요");
    }

    #[tokio::test]
    async fn http_disconnect_cancels_work() {
        let url = start(None, vec![]).await;
        let c = client();
        let sid = init(&c, &url, None).await;
        let tag = "cancel-drop";
        let resp = post(
            &c,
            &url,
            json!({"jsonrpc":"2.0","id":31,"method":"tools/call","params":{
                "name":"test_sleep","arguments":{"tag":tag},"_meta":{"progressToken":1}}}),
        )
        .header(SESSION_HEADER, &sid)
        .send()
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(resp);
        drop(c);
        for _ in 0..50 {
            if crate::test_tools::aborted(tag) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("연결이 끊겼는데 작업이 멈추지 않았어요");
    }

    fn feats(n: usize) -> Vec<Value> {
        (0..n)
            .map(|i| json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": [127.0, 37.5]},
                "properties": {"pnu": format!("{:019}", i), "addr": "서울특별시 중구 태평로1가"}}))
            .collect()
    }

    #[tokio::test]
    async fn http_results_download_needs_token() {
        let store = test_store();
        let meta = store
            .save("vworld_data", &json!({}), &feats(3), true)
            .unwrap();
        let id = meta["id"].as_str().unwrap().to_string();
        let url = start_with(Some("tok-123"), vec![], store.clone()).await;
        let base = url.trim_end_matches("/mcp");
        let c = client();
        let r = c
            .get(format!("{base}/results/{id}.csv"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
        let r = c
            .get(format!("{base}/results/{id}.csv"))
            .bearer_auth("nope")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
        let r = c
            .get(format!("{base}/results/{id}.csv"))
            .bearer_auth("tok-123")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/csv"));
        assert!(r.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .contains(&format!("{id}.csv")));
        let len: usize = r.headers()["content-length"]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        let body = r.text().await.unwrap();
        assert_eq!(body.len(), len);
        assert_eq!(body.lines().count(), 4);
        let r = c
            .get(format!("{base}/results/{id}.geojson"))
            .bearer_auth("tok-123")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let r = c
            .get(format!("{base}/results/{id}.jsonl"))
            .bearer_auth("tok-123")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404);
        let r = c
            .get(format!("{base}/results/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA.csv"))
            .bearer_auth("tok-123")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404);
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[tokio::test]
    async fn http_result_page_and_resources_read_without_keys() {
        let store = test_store();
        let shaped = vworld::mcp_results::shape_result(
            &store,
            "vworld_data",
            &json!({}),
            &json!({}),
            Some((feats(30), true)),
            &json!({}),
            &vworld::mcp_results::Shape {
                save: true,
                format: Some(vworld::mcp_results::Format::Csv),
                ..Default::default()
            },
        )
        .unwrap();
        let first: Value = serde_json::from_str(&shaped.text).unwrap();
        let id = first["result_id"].as_str().unwrap().to_string();
        let cursor =
            vworld::mcp_results::encode_cursor(&id, 10, vworld::mcp_results::Format::Csv, None);
        let url = start_with(None, vec![], store.clone()).await; // 기본 키 없음
        let c = client();
        let r = post(&c, &url, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}))
            .send().await.unwrap();
        let sid = r.headers()[SESSION_HEADER].to_str().unwrap().to_string();
        let v: Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
        assert!(
            v["result"]["capabilities"]["resources"].is_object(),
            "resources capability"
        );
        // 키 없이 다음 쪽.
        let r = call(
            &c,
            &url,
            &sid,
            2,
            "vworld_result_page",
            json!({"cursor": cursor, "limit": 5}),
        )
        .await;
        let v: Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
        assert_ne!(v["result"]["isError"], true, "{v}");
        let page: Value =
            serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(page["returned_rows"], 5);
        assert_eq!(page["total_rows"], 30);
        assert!(page["data"].as_str().unwrap().starts_with("addr,pnu\n"));
        let link = &v["result"]["content"][1];
        assert_eq!(link["type"], "resource_link");
        // resources/read
        let r = post(
            &c,
            &url,
            json!({"jsonrpc":"2.0","id":3,"method":"resources/read",
            "params":{"uri": format!("vworld://results/{id}.csv")}}),
        )
        .header(SESSION_HEADER, &sid)
        .send()
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
        assert_eq!(v["result"]["contents"][0]["mimeType"], "text/csv");
        assert_eq!(
            v["result"]["contents"][0]["text"]
                .as_str()
                .unwrap()
                .lines()
                .count(),
            31
        );
        // 만료(없는) 결과.
        let gone = vworld::mcp_results::encode_cursor(
            &vworld::mcp_results::new_id(),
            0,
            vworld::mcp_results::Format::Json,
            None,
        );
        let r = call(
            &c,
            &url,
            &sid,
            4,
            "vworld_result_page",
            json!({"cursor": gone}),
        )
        .await;
        let v: Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true);
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("만료"));
        let _ = std::fs::remove_dir_all(&store.dir);
    }
}
