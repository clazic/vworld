//! HTTP 공통 계층 — 클라이언트, 요청 빌더 3분법, 200 본문 에러 검사, 응답 정규화(설계 §2.1.1·§1.3-G1).

pub mod normalize;

use crate::concurrency::retry::{classify_reqwest, parse_retry_after, RateLimited, RetryPolicy};
use crate::concurrency::{FailKind, KeyUsage};
use crate::relay::{Relay, RelayError};
use anyhow::{anyhow, Result};
use std::time::Duration;

/// VWorld REST 베이스 호스트.
pub const REQ_BASE: &str = "https://api.vworld.kr/req";
pub const NED_BASE: &str = "https://api.vworld.kr/ned";

/// VWorld API 에러(HTTP 200 본문에 담기는 경우 포함).
#[derive(Debug, thiserror::Error)]
#[error("VWorld API 오류 [{code}]: {text}")]
pub struct ApiError {
    pub code: String,
    pub text: String,
    /// "데이터 없음" 류 — 배치에서 빈 결과로 정상 처리.
    pub empty_ok: bool,
}

/// 쿼리형 요청 빌더 — `/req/{service}` + 공통 파라미터(설계 §1.3-결정2 QueryBuilder).
#[derive(Debug, Clone)]
pub struct QueryBuilder {
    service: String,
    params: Vec<(String, String)>,
}

impl QueryBuilder {
    /// 쿼리형 서비스 시작. `service`/`request`/`version`을 부착(format은 호출자가 지정).
    ///
    /// format을 자동 주입하지 않는 이유: 데이터형(json/xml)과 이미지형(png/jpeg/bmp)이 달라
    /// 중복·충돌을 피하기 위해 호출자가 `.format()`으로 명시한다.
    pub fn new(service: &str, request: &str) -> Self {
        let mut b = QueryBuilder {
            service: service.to_string(),
            params: Vec::new(),
        };
        b.params.push(("service".into(), service.to_string()));
        b.params.push(("request".into(), request.to_string()));
        b.params.push(("version".into(), "2.0".into()));
        b
    }

    /// 응답 포맷 지정(데이터형 "json"/"xml", 이미지형 "png"/"jpeg"/"bmp").
    pub fn format(mut self, fmt: &str) -> Self {
        self.params.push(("format".into(), fmt.to_string()));
        self
    }

    /// version 교체(기본 2.0). WMS/WFS는 OGC 표준 버전(WFS 1.1.0 등)이 필요.
    pub fn version(mut self, v: &str) -> Self {
        if let Some(p) = self.params.iter_mut().find(|p| p.0 == "version") {
            p.1 = v.to_string();
        }
        self
    }

    /// 선택 파라미터 추가(값이 Some일 때만).
    pub fn opt(mut self, key: &str, value: Option<&str>) -> Self {
        if let Some(v) = value {
            self.params.push((key.to_string(), v.to_string()));
        }
        self
    }

    /// 필수/임의 파라미터 추가.
    pub fn set(mut self, key: &str, value: &str) -> Self {
        self.params.push((key.to_string(), value.to_string()));
        self
    }

    /// 완성된 (base_url, query params). `key`/`domain`은 Client가 주입.
    pub fn build(self) -> (String, Vec<(String, String)>) {
        let url = format!("{REQ_BASE}/{}", self.service);
        (url, self.params)
    }
}

/// NED 요청 빌더 — `/ned/{kind}/{op}`, 계열별 JSON 강제 파라미터 분기(설계 §1.1.2·§1.3 NedBuilder).
#[derive(Debug, Clone)]
pub struct NedBuilder {
    kind: NedKind,
    op: String,
    params: Vec<(String, String)>,
}

/// NED 계열.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NedKind {
    Wms,
    Wfs,
    Data,
}

impl NedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NedKind::Wms => "wms",
            NedKind::Wfs => "wfs",
            NedKind::Data => "data",
        }
    }
}

impl NedBuilder {
    pub fn new(kind: NedKind, op: &str) -> Self {
        let mut b = NedBuilder {
            kind,
            op: op.to_string(),
            params: Vec::new(),
        };
        // 계열별 JSON 강제 파라미터(설계 §1.1.2-1): WFS=output, data=format. WMS는 이미지.
        match kind {
            NedKind::Wfs => b.params.push(("output".into(), "application/json".into())),
            NedKind::Data => b.params.push(("format".into(), "json".into())),
            NedKind::Wms => {}
        }
        b
    }

    pub fn set(mut self, key: &str, value: &str) -> Self {
        self.params.push((key.to_string(), value.to_string()));
        self
    }

    pub fn opt(mut self, key: &str, value: Option<&str>) -> Self {
        if let Some(v) = value {
            self.params.push((key.to_string(), v.to_string()));
        }
        self
    }

    pub fn build(self) -> (String, Vec<(String, String)>) {
        let url = format!("{NED_BASE}/{}/{}", self.kind.as_str(), self.op);
        (url, self.params)
    }
}

/// 타일 경로형 빌더 — `/req/{wmts|tms}/1.0.0/{layer}/{z}/{row}/{col}.{ext}`(설계 §1.3 PathBuilder).
#[derive(Debug, Clone)]
pub struct PathBuilder {
    path: String,
}

impl PathBuilder {
    /// WMTS/TMS REST 타일 경로 — **key가 경로에 포함**(쿼리 아님).
    /// 실제 템플릿: `/req/{scheme}/1.0.0/{key}/{layer}/{z}/{row}/{col}.{ext}`.
    pub fn tile(scheme: &str, key: &str, layer: &str, z: u32, row: u32, col: u32, ext: &str) -> Self {
        PathBuilder {
            path: format!("{REQ_BASE}/{scheme}/1.0.0/{key}/{layer}/{z}/{row}/{col}.{ext}"),
        }
    }

    /// WMTS 해외위성영상 시계열 타일 경로 — key가 경로에 포함.
    /// 템플릿: `/req/wmts/1.0.0/{key}/Satellite/themes/{category}/{year}/{city}/{z}/{row}/{col}.{ext}`.
    #[allow(clippy::too_many_arguments)]
    pub fn wmts_themes(
        key: &str,
        category: &str,
        year: &str,
        city: &str,
        z: u32,
        row: u32,
        col: u32,
        ext: &str,
    ) -> Self {
        PathBuilder {
            path: format!(
                "{REQ_BASE}/wmts/1.0.0/{key}/Satellite/themes/{category}/{year}/{city}/{z}/{row}/{col}.{ext}"
            ),
        }
    }

    /// WMTS GetCapabilities 경로 — 타일행렬셋·레이어 메타데이터(XML).
    /// 템플릿: `/req/wmts/1.0.0/{key}/WMTSCapabilities.xml`.
    pub fn wmts_capabilities(key: &str) -> Self {
        PathBuilder {
            path: format!("{REQ_BASE}/wmts/1.0.0/{key}/WMTSCapabilities.xml"),
        }
    }

    /// 벡터타일 경로(getTile/getStyle) — key가 경로에 포함.
    /// 템플릿: `/req/wmts/vector/{op}/{key}/{rest}`.
    pub fn vector(op: &str, key: &str, rest: &str) -> Self {
        PathBuilder {
            path: format!("{REQ_BASE}/wmts/vector/{op}/{key}/{rest}"),
        }
    }

    /// 벡터 래스터 PNG 경로 — key가 먼저, getTile 없음.
    /// 템플릿: `/req/wmts/vector/{key}/{layer}/{z}/{row}/{col}.{ext}`.
    pub fn vector_raster(key: &str, layer: &str, z: u32, row: u32, col: u32, ext: &str) -> Self {
        PathBuilder {
            path: format!("{REQ_BASE}/wmts/vector/{key}/{layer}/{z}/{row}/{col}.{ext}"),
        }
    }

    /// 완성된 URL(타일은 key가 경로에 있으므로 쿼리·인증 미주입).
    pub fn url(self) -> String {
        self.path
    }
}

/// WMTS row → TMS row 변환(설계 §1.3-결정1 골든 공식). Y축 반전.
pub fn wmts_row_to_tms(z: u32, wmts_row: u32) -> u32 {
    (1u32 << z) - 1 - wmts_row
}

/// 공유 HTTP 클라이언트(connection pool, HTTP/2, timeout).
#[derive(Clone)]
pub struct Client {
    inner: reqwest::Client,
    /// PES 중계 모드(`vworld` CLI 전용). Some이면 모든 요청이 중계 주소로 나간다.
    relay: Option<std::sync::Arc<Relay>>,
    /// 키별 사용량 세기(`vworld-mcp` 의 요청별 키 호출 전용). None 이면 세지 않는다.
    usage: Option<std::sync::Arc<KeyUsage>>,
    /// VWorld 호스트(`https://api.vworld.kr`) 대신 쓸 주소 — 시험용 흉내 서버.
    upstream: Option<std::sync::Arc<str>>,
}

/// VWorld API 호스트(스킴 포함).
const API_ORIGIN: &str = "https://api.vworld.kr";

/// 중계 모드에서 따라갈 넘김(3xx) 횟수 상한(reqwest 기본값과 같음).
const MAX_RELAY_REDIRECTS: u32 = 10;

/// 호출 시 주입할 인증 컨텍스트.
#[derive(Debug, Clone, Default)]
pub struct Auth {
    pub key: String,
    /// 도메인 등록 키 대응(`domain=` 쿼리 + Referer 헤더).
    pub domain: Option<String>,
}

impl Client {
    pub fn new() -> Result<Self> {
        let inner = reqwest::Client::builder()
            .pool_max_idle_per_host(8)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("vworld-cli/0.1")
            .build()?;
        Ok(Client { inner, relay: None, usage: None, upstream: None })
    }

    /// `vworld` CLI용 클라이언트 — `PES_RELAY_URL` 이 있으면 중계 모드, 없으면 `new()`와 같다.
    pub fn for_cli() -> Result<Self> {
        match Relay::from_env() {
            Some(relay) => Self::with_relay(relay),
            None => Self::new(),
        }
    }

    /// 중계 모드 클라이언트. 모든 요청을 `{중계}/vworld/<scheme>/<host>/<path>?<query>` 로 보낸다.
    ///
    /// - 넘김(3xx)은 자동으로 따라가지 않는다 — `Location` 을 다시 중계 주소로 바꿔 요청한다.
    /// - 중계는 `127.0.0.1` 이라 시스템 프록시를 거치지 않는다.
    pub fn with_relay(relay: Relay) -> Result<Self> {
        let inner = reqwest::Client::builder()
            .pool_max_idle_per_host(8)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("vworld-cli/0.1")
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()?;
        Ok(Client { inner, relay: Some(std::sync::Arc::new(relay)), usage: None, upstream: None })
    }

    /// 이 호출 동안 키별 사용량을 `usage` 에 세는 사본(연결 풀은 함께 쓴다).
    pub fn with_usage(&self, usage: std::sync::Arc<KeyUsage>) -> Self {
        let mut c = self.clone();
        c.usage = Some(usage);
        c
    }

    /// 시험용: `https://api.vworld.kr` 로 갈 요청을 `base`(흉내 서버)로 보낸다.
    #[doc(hidden)]
    pub fn with_upstream(mut self, base: &str) -> Self {
        self.upstream = Some(base.trim_end_matches('/').into());
        self
    }

    /// HTTP 200 으로 받은 본문이 오류였다(VWorld 오류 본문 · 읽기 실패) — 사용량에 실패로 센다.
    /// `limited` 는 사용량 초과 오류일 때.
    pub fn note_body_error(&self, key: &str, limited: bool) {
        if let Some(u) = &self.usage {
            u.record_error(key, limited);
        }
    }

    /// 인증 파라미터(key/domain)를 주입.
    fn with_auth(mut params: Vec<(String, String)>, auth: &Auth) -> Vec<(String, String)> {
        params.push(("key".into(), auth.key.clone()));
        if let Some(d) = &auth.domain {
            params.push(("domain".into(), d.clone()));
        }
        params
    }

    /// 재시도 포함 GET 코어. `referer`가 Some이면 헤더 주입.
    /// `key` 는 이 요청에 실린 키(사용량 세기용 — 키가 경로에 든 요청은 None, 세지 않는다).
    async fn send_retry(
        &self,
        url: &str,
        params: &[(String, String)],
        referer: Option<&str>,
        key: Option<&str>,
    ) -> Result<reqwest::Response> {
        // 사용량: 실제로 보낸 요청마다(재시도 포함) 한 번씩 센다.
        let usage = self.usage.as_deref().zip(key);
        let policy = RetryPolicy::default();
        let mut attempt = 0u32;
        // 중계 모드: 주소를 중계 주소로 바꾼다(유일한 길목). 넘김을 받으면 `target` 이 바뀐다.
        let relay = self.relay.as_deref();
        let mut target = match relay {
            Some(r) => r.wrap(url)?,
            None => match (&self.upstream, url.strip_prefix(API_ORIGIN)) {
                (Some(base), Some(rest)) => format!("{base}{rest}"),
                _ => url.to_string(),
            },
        };
        let mut query: &[(String, String)] = params;
        let mut redirects = 0u32;
        loop {
            if relay.is_some() {
                // 중계 오류를 한 번 받았으면 더 보내지 않는다(다른 키로 다시 해 보지 않는다).
                if let Some(e) = crate::relay::recorded_error() {
                    return Err(e.into());
                }
            }
            let mut req = self.inner.get(&target).query(query);
            if let Some(d) = referer {
                req = req.header(reqwest::header::REFERER, d);
            }
            if let Some(r) = relay {
                req = req.header(crate::relay::TOKEN_HEADER, &r.token);
            }
            if let Some((u, k)) = usage {
                u.record_call(k);
            }
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if let Some((u, k)) = usage {
                        if status.is_client_error() || status.is_server_error() {
                            u.record_error(k, status.as_u16() == 429);
                        }
                    }
                    if let Some(r) = relay {
                        // 중계가 만든 오류(401 · 403 · 409)는 기관 응답이 아니다 — 글 그대로 보여 주고 끝.
                        if resp.headers().contains_key(crate::relay::ERROR_HEADER) {
                            let body = resp.text().await.unwrap_or_default();
                            let text = crate::relay::error_text(status.as_u16(), &body);
                            crate::relay::record_error(&text);
                            return Err(RelayError(text).into());
                        }
                        // 넘김: 중계는 따라가지 않고 그대로 준다 — `Location` 을 다시 중계 주소로.
                        if status.is_redirection() {
                            if let Some(loc) = resp
                                .headers()
                                .get(reqwest::header::LOCATION)
                                .and_then(|v| v.to_str().ok())
                            {
                                if redirects >= MAX_RELAY_REDIRECTS {
                                    return Err(anyhow!("넘김(redirect)이 너무 많습니다(HTTP {status})"));
                                }
                                // 현재 주소(질의 포함)를 기준으로 푼다. `Location` 에 질의가 다 들어 있다.
                                let current = resp.url().to_string();
                                target = r.wrap_location(&current, loc)?;
                                query = &[];
                                redirects += 1;
                                continue;
                            }
                        }
                    }
                    if status.is_success() {
                        return Ok(resp);
                    }
                    let retry_after = parse_retry_after(
                        resp.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()),
                    );
                    if should_retry_status(status.as_u16()) && attempt < policy.max_retry {
                        // 429는 서버 Retry-After를 존중(30초 상한), 그 외는 지수 backoff.
                        let wait = match retry_after {
                            Some(d) if status.as_u16() == 429 => d.min(Duration::from_secs(30)),
                            _ => policy.backoff(attempt),
                        };
                        tokio::time::sleep(wait).await;
                        attempt += 1;
                        continue;
                    }
                    if status.as_u16() == 429 {
                        return Err(RateLimited { retry_after }.into());
                    }
                    let body = resp.text().await.unwrap_or_default();
                    return Err(anyhow!("HTTP {status}: {}", truncate(&body, 300)));
                }
                Err(e) => {
                    if let Some((u, k)) = usage {
                        u.record_error(k, false);
                    }
                    if classify_reqwest(&e) == FailKind::Transient && attempt < policy.max_retry {
                        tokio::time::sleep(policy.backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    // without_url: reqwest 에러 Display에 key가 담긴 URL이 포함됨 — 노출 차단.
                    return Err(e.without_url().into());
                }
            }
        }
    }

    /// 텍스트(JSON/XML/HTML) 응답 GET — key/domain 주입 + Referer 헤더 + 재시도(§2.1.1-c).
    pub async fn get_text(&self, url: &str, params: Vec<(String, String)>, auth: &Auth) -> Result<String> {
        let params = Self::with_auth(params, auth);
        let resp = self.send_retry(url, &params, auth.domain.as_deref(), Some(&auth.key)).await?;
        Ok(resp.text().await.map_err(|e| {
            self.note_body_error(&auth.key, false);
            e.without_url()
        })?)
    }

    /// 인증 미주입 텍스트 GET — key가 경로에 포함된 타일 등에 사용.
    pub async fn get_text_plain(&self, url: &str) -> Result<String> {
        let resp = self.send_retry(url, &[], None, None).await?;
        Ok(resp.text().await.map_err(|e| e.without_url())?)
    }

    /// `apiKey=` 방식 텍스트 GET — apis.vworld.kr Geocoder 등(`key=` 아님).
    pub async fn get_text_apikey(
        &self,
        url: &str,
        mut params: Vec<(String, String)>,
        key: &str,
    ) -> Result<String> {
        params.push(("apiKey".into(), key.to_string()));
        let resp = self.send_retry(url, &params, None, Some(key)).await?;
        Ok(resp.text().await.map_err(|e| {
            self.note_body_error(key, false);
            e.without_url()
        })?)
    }

    /// 바이트(이미지/타일) 응답 GET — key/domain 주입 + 재시도 + **본문 에러 검사**(§2.1.1-b).
    pub async fn get_bytes(&self, url: &str, params: Vec<(String, String)>, auth: &Auth) -> Result<Vec<u8>> {
        let params = Self::with_auth(params, auth);
        let resp = self.send_retry(url, &params, auth.domain.as_deref(), Some(&auth.key)).await?;
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| {
                self.note_body_error(&auth.key, false);
                e.without_url()
            })?
            .to_vec();
        guard_image_error(&bytes).inspect_err(|_| self.note_body_error(&auth.key, false))?;
        Ok(bytes)
    }

    /// 인증 미주입 바이트 GET(타일 — key 경로 포함) + 본문 에러 검사.
    pub async fn get_bytes_plain(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.send_retry(url, &[], None, None).await?;
        let bytes = resp.bytes().await.map_err(|e| e.without_url())?.to_vec();
        guard_image_error(&bytes)?;
        Ok(bytes)
    }
}

/// 이미지로 받았으나 본문이 JSON/XML 에러(HTTP 200)인 경우 에러로 승격(§2.1.1-b).
fn guard_image_error(bytes: &[u8]) -> Result<()> {
    // 알려진 이미지 매직(PNG/JPEG/BMP/GIF)이면 정상.
    let is_image = bytes.starts_with(&[0x89, b'P', b'N', b'G'])
        || bytes.starts_with(&[0xFF, 0xD8]) // JPEG
        || bytes.starts_with(b"BM") // BMP
        || bytes.starts_with(b"GIF");
    if is_image {
        return Ok(());
    }
    // 텍스트(JSON/XML)면 에러 본문일 가능성 — 파싱해서 메시지 추출.
    let head: String = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]).trim_start().to_string();
    if head.starts_with('{') || head.starts_with('<') {
        return Err(anyhow!("이미지 응답이 아닌 에러 본문 수신: {}", truncate(&head, 300)));
    }
    // 매직은 모르지만 바이너리일 수 있음(예: MVT) — 통과.
    Ok(())
}

/// 5xx/429는 재시도 대상.
fn should_retry_status(code: u16) -> bool {
    code == 429 || (500..600).contains(&code)
}

// Format::Xml은 --raw XML 원문 요청용 공개 옵션(현재 경로는 JSON 우선).

fn truncate(s: &str, n: usize) -> String {
    // 문자 경계에서 자름(바이트 슬라이스는 한글 중간에서 panic).
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// `--param k=v` 패스스루 파싱 — `key`/`domain` 충돌 거부(설계 §8-⑨).
pub fn parse_passthrough(items: &[String]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for it in items {
        let (k, v) = it
            .split_once('=')
            .ok_or_else(|| anyhow!("--param 형식 오류(k=v 필요): {it}"))?;
        let k = k.trim();
        if k.eq_ignore_ascii_case("key") || k.eq_ignore_ascii_case("domain") {
            return Err(anyhow!(
                "--param으로 '{k}'를 지정할 수 없습니다(인증은 config/--referer로 주입)"
            ));
        }
        out.push((k.to_string(), v.to_string()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_char_boundary() {
        assert_eq!(truncate("가나다라", 2), "가나…");
        assert_eq!(truncate("가나", 5), "가나");
    }

    #[test]
    fn tms_y_flip_golden() {
        // z=6 → 2^6-1=63. wmts_row=10 → tms_row=53.
        assert_eq!(wmts_row_to_tms(6, 10), 53);
        // z=0 → 1-1=0, row 0 → 0.
        assert_eq!(wmts_row_to_tms(0, 0), 0);
        // z=18 경계.
        assert_eq!(wmts_row_to_tms(18, 0), (1u32 << 18) - 1);
        // 왕복 항등: tms(tms(x)) == x.
        let r = wmts_row_to_tms(10, 300);
        assert_eq!(wmts_row_to_tms(10, r), 300);
    }

    #[test]
    fn query_builder_common_params() {
        let (url, params) = QueryBuilder::new("address", "GetCoord")
            .format("json")
            .set("type", "ROAD")
            .opt("crs", Some("EPSG:4326"))
            .opt("refine", None)
            .build();
        assert_eq!(url, "https://api.vworld.kr/req/address");
        assert!(params.iter().any(|(k, v)| k == "request" && v == "GetCoord"));
        assert!(params.iter().any(|(k, v)| k == "format" && v == "json"));
        assert!(params.iter().any(|(k, v)| k == "type" && v == "ROAD"));
        assert!(params.iter().any(|(k, _)| k == "crs"));
        assert!(!params.iter().any(|(k, _)| k == "refine"));
        // format 자동 중복 주입 없음(이미지형 충돌 회귀 방지).
        assert_eq!(params.iter().filter(|(k, _)| k == "format").count(), 1);
    }

    #[test]
    fn ned_builder_kind_json_param() {
        let (url, params) = NedBuilder::new(NedKind::Wfs, "getBuildingAgeWFS").build();
        assert_eq!(url, "https://api.vworld.kr/ned/wfs/getBuildingAgeWFS");
        assert!(params.iter().any(|(k, v)| k == "output" && v == "application/json"));

        let (url, params) = NedBuilder::new(NedKind::Data, "getBuildingAge").build();
        assert_eq!(url, "https://api.vworld.kr/ned/data/getBuildingAge");
        assert!(params.iter().any(|(k, v)| k == "format" && v == "json"));

        let (_, params) = NedBuilder::new(NedKind::Wms, "getBuildingAgeWMS").build();
        assert!(!params.iter().any(|(k, _)| k == "output" || k == "format"));
    }

    #[test]
    fn path_builder_tile_url() {
        // key가 경로에 포함되는 실제 템플릿.
        let url = PathBuilder::tile("wmts", "MYKEY", "Base", 10, 200, 300, "png").url();
        assert_eq!(url, "https://api.vworld.kr/req/wmts/1.0.0/MYKEY/Base/10/200/300.png");
        let vurl = PathBuilder::vector("getTile", "MYKEY", "poi/11/1746/793.pbf").url();
        assert_eq!(vurl, "https://api.vworld.kr/req/wmts/vector/getTile/MYKEY/poi/11/1746/793.pbf");
    }

    #[test]
    fn path_builder_wmts_themes_and_capabilities() {
        // 해외위성영상 시계열(가이드 예: /Satellite/themes/cities/2025/Oslo/11/1086/596.png).
        let turl = PathBuilder::wmts_themes("MYKEY", "cities", "2025", "Oslo", 11, 1086, 596, "png").url();
        assert_eq!(
            turl,
            "https://api.vworld.kr/req/wmts/1.0.0/MYKEY/Satellite/themes/cities/2025/Oslo/11/1086/596.png"
        );
        // GetCapabilities(XML 메타데이터).
        let curl = PathBuilder::wmts_capabilities("MYKEY").url();
        assert_eq!(curl, "https://api.vworld.kr/req/wmts/1.0.0/MYKEY/WMTSCapabilities.xml");
    }

    #[test]
    fn passthrough_rejects_auth_keys() {
        assert!(parse_passthrough(&["foo=bar".into()]).is_ok());
        assert!(parse_passthrough(&["key=leak".into()]).is_err());
        assert!(parse_passthrough(&["DOMAIN=x".into()]).is_err());
        assert!(parse_passthrough(&["noeq".into()]).is_err());
    }
}
