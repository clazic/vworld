//! PES 로컬 중계 모드 — 키 없이 자리표시로 요청하고, 브리지가 연 `127.0.0.1` 중계가 실제 키를 끼운다.
//!
//! 규약: pes-bridge `RELAY.md`. 환경 변수 `PES_RELAY_URL` 이 있으면 중계 모드다.
//! - 키 풀 = 자리표시 `__PES_KEY_0__` … `__PES_KEY_{n-1}__` (n = `PES_RELAY_VWORLD_KEYS`)
//! - 도메인(`domain=` · Referer) = `__PES_DOMAIN__`
//! - 주소 `<scheme>://<host>/<path>?<query>` → `{PES_RELAY_URL}/vworld/<scheme>/<host>/<path>?<query>`
//! - 머리글 `X-PES-Relay-Token` 추가, 중계 오류(`X-PES-Relay-Error: 1`)는 `error` 글 그대로 보여 주고 끝.
//!
//! `vworld` CLI 전용이다. `vworld-mcp` 는 이 모듈을 쓰지 않는다(키를 직접 받는다).

use std::sync::Mutex;

/// 중계 주소 환경 변수(있으면 중계 모드).
pub const ENV_URL: &str = "PES_RELAY_URL";
/// 이번 실행에만 통하는 토큰.
pub const ENV_TOKEN: &str = "PES_RELAY_TOKEN";
/// PES 에 등록된 VWorld 키 수.
pub const ENV_KEYS: &str = "PES_RELAY_VWORLD_KEYS";

/// 중계 경로의 도구 이름.
pub const TOOL: &str = "vworld";
/// 등록 도메인 자리표시(중계가 실제 도메인으로 바꾼다).
pub const DOMAIN_PLACEHOLDER: &str = "__PES_DOMAIN__";
/// 토큰 머리글.
pub const TOKEN_HEADER: &str = "X-PES-Relay-Token";
/// 중계가 만든 오류 응답 표시 머리글.
pub const ERROR_HEADER: &str = "X-PES-Relay-Error";
/// 키 수가 0 이거나 변수가 없을 때 알림(규약 문구 그대로).
pub const NO_KEYS_MESSAGE: &str =
    "PES 에 VWorld 키가 없어요. PES 「조사 도구 · 그림 키」에서 키를 넣으세요.";

/// 중계 설정(비밀 아님 — 키 없음).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relay {
    /// 중계 주소(끝 `/` 없음). 예: `http://127.0.0.1:53999`.
    pub base: String,
    pub token: String,
    /// PES 에 등록된 VWorld 키 수.
    pub n_keys: usize,
}

/// 중계가 준 오류(토큰 틀림 401 · 허용 안 된 호스트 403 · 키 없음 409). 기관 응답이 아니다.
#[derive(Debug, Clone)]
pub struct RelayError(pub String);

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RelayError {}

impl Relay {
    /// 환경 변수에서 읽는다. `PES_RELAY_URL` 이 없거나 비면 `None`(= 평소 동작).
    pub fn from_env() -> Option<Relay> {
        Self::from_values(
            std::env::var(ENV_URL).ok().as_deref(),
            std::env::var(ENV_TOKEN).ok().as_deref(),
            std::env::var(ENV_KEYS).ok().as_deref(),
        )
    }

    /// 값으로 만든다(시험용 진입점 겸용). 키 수가 숫자가 아니면 0 으로 본다.
    pub fn from_values(url: Option<&str>, token: Option<&str>, keys: Option<&str>) -> Option<Relay> {
        let base = url?.trim().trim_end_matches('/');
        if base.is_empty() {
            return None;
        }
        Some(Relay {
            base: base.to_string(),
            token: token.unwrap_or("").trim().to_string(),
            n_keys: keys.and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(0),
        })
    }

    /// 자리표시 키 풀 — `__PES_KEY_0__` … `__PES_KEY_{n-1}__`.
    pub fn placeholder_keys(&self) -> Vec<String> {
        (0..self.n_keys).map(key_placeholder).collect()
    }

    /// 이미 중계로 가는 주소인지.
    pub fn is_relay_url(&self, url: &str) -> bool {
        url == self.base
            || url
                .strip_prefix(&self.base)
                .is_some_and(|rest| rest.starts_with('/') || rest.starts_with('?'))
    }

    /// 기관 주소를 중계 주소로 바꾼다. 이미 중계 주소면 그대로.
    ///
    /// `https://api.vworld.kr/req/address?x=1` → `{base}/vworld/https/api.vworld.kr/req/address?x=1`
    pub fn wrap(&self, url: &str) -> Result<String, RelayError> {
        if self.is_relay_url(url) {
            return Ok(url.to_string());
        }
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| RelayError(format!("중계로 보낼 수 없는 주소입니다: {url}")))?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(RelayError(format!("중계로 보낼 수 없는 주소입니다: {url}")));
        }
        Ok(format!("{}/{TOOL}/{scheme}/{rest}", self.base))
    }

    /// 중계 주소를 원래 기관 주소로 되돌린다(넘김 `Location` 이 상대 경로일 때 기준 주소로 쓴다).
    pub fn unwrap_url(&self, url: &str) -> Option<String> {
        let rest = url.strip_prefix(&self.base)?.strip_prefix('/')?;
        let rest = rest.strip_prefix(TOOL)?.strip_prefix('/')?;
        let (scheme, rest) = rest.split_once('/')?;
        Some(format!("{scheme}://{rest}"))
    }

    /// 넘김(3xx) `Location` 을 다음에 요청할 중계 주소로 바꾼다.
    ///
    /// `current` 는 방금 요청한 중계 주소. `Location` 이 기관 절대 주소면 다시 감싸고,
    /// 상대 경로면 원래 기관 주소 기준으로 풀어서 감싼다. 중계 밖으로 직접 나가지 않는다.
    pub fn wrap_location(&self, current: &str, location: &str) -> Result<String, RelayError> {
        let location = location.trim();
        if location.contains("://") {
            return self.wrap(location);
        }
        let upstream = self
            .unwrap_url(current)
            .ok_or_else(|| RelayError(format!("넘김 주소를 풀 수 없습니다: {location}")))?;
        let (scheme, rest) = upstream.split_once("://").unwrap_or(("https", &upstream));
        // 질의 · 조각을 뗀 host[/path].
        let no_query = rest.split(['?', '#']).next().unwrap_or(rest);
        let (host, path) = match no_query.find('/') {
            Some(i) => (&no_query[..i], &no_query[i..]),
            None => (no_query, "/"),
        };
        let target = if let Some(net) = location.strip_prefix("//") {
            format!("{scheme}://{net}")
        } else if location.starts_with('/') {
            format!("{scheme}://{host}{location}")
        } else if location.starts_with('?') {
            format!("{scheme}://{host}{path}{location}")
        } else {
            let dir = &path[..path.rfind('/').map(|i| i + 1).unwrap_or(0)];
            format!("{scheme}://{host}{dir}{location}")
        };
        self.wrap(&target)
    }
}

/// i 번째 키 자리표시.
pub fn key_placeholder(i: usize) -> String {
    format!("__PES_KEY_{i}__")
}

/// 중계 오류 본문(JSON `{"error":"…"}`)에서 보여 줄 글을 꺼낸다. JSON 이 아니면 본문 그대로.
pub fn error_text(status: u16, body: &str) -> String {
    let text = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_else(|| body.trim().to_string());
    if text.is_empty() {
        format!("PES 중계 오류 (HTTP {status})")
    } else {
        text
    }
}

/// 이번 실행에서 처음 받은 중계 오류. 한 번 받으면 남은 요청은 중계로 보내지 않는다
/// (다른 키로 다시 해 보지 않는다 — 규약).
static FIRST_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// 중계 오류를 적어 둔다(처음 것만).
pub fn record_error(text: &str) {
    let mut slot = FIRST_ERROR.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        *slot = Some(text.to_string());
    }
}

/// 적어 둔 중계 오류.
pub fn recorded_error() -> Option<RelayError> {
    FIRST_ERROR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .map(RelayError)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay() -> Relay {
        Relay::from_values(Some("http://127.0.0.1:53999/"), Some(" tok "), Some("3")).unwrap()
    }

    #[test]
    fn off_without_url() {
        assert!(Relay::from_values(None, Some("t"), Some("2")).is_none());
        assert!(Relay::from_values(Some("  "), Some("t"), Some("2")).is_none());
    }

    #[test]
    fn reads_values_and_trims() {
        let r = relay();
        assert_eq!(r.base, "http://127.0.0.1:53999");
        assert_eq!(r.token, "tok");
        assert_eq!(r.n_keys, 3);
        assert_eq!(r.placeholder_keys(), vec!["__PES_KEY_0__", "__PES_KEY_1__", "__PES_KEY_2__"]);
    }

    #[test]
    fn missing_or_bad_key_count_is_zero() {
        assert_eq!(Relay::from_values(Some("http://h"), None, None).unwrap().n_keys, 0);
        assert_eq!(Relay::from_values(Some("http://h"), None, Some("x")).unwrap().n_keys, 0);
        assert_eq!(Relay::from_values(Some("http://h"), None, Some("-1")).unwrap().n_keys, 0);
        assert!(Relay::from_values(Some("http://h"), None, Some("0")).unwrap().placeholder_keys().is_empty());
    }

    #[test]
    fn wraps_upstream_urls() {
        let r = relay();
        assert_eq!(
            r.wrap("https://api.vworld.kr/req/address?a=1&b=2").unwrap(),
            "http://127.0.0.1:53999/vworld/https/api.vworld.kr/req/address?a=1&b=2"
        );
        assert_eq!(
            r.wrap("http://apis.vworld.kr:8080/new2coord.do").unwrap(),
            "http://127.0.0.1:53999/vworld/http/apis.vworld.kr:8080/new2coord.do"
        );
        // 이미 중계 주소면 두 번 감싸지 않는다.
        let once = r.wrap("https://api.vworld.kr/req/wms").unwrap();
        assert_eq!(r.wrap(&once).unwrap(), once);
        assert!(r.wrap("ftp://api.vworld.kr/x").is_err());
        assert!(r.wrap("api.vworld.kr/x").is_err());
        // 포트만 앞이 같은 다른 주소는 중계 주소가 아니다.
        assert!(!r.is_relay_url("http://127.0.0.1:539990/x"));
    }

    #[test]
    fn unwraps_back_to_upstream() {
        let r = relay();
        let w = r.wrap("https://api.vworld.kr/req/data?x=1").unwrap();
        assert_eq!(r.unwrap_url(&w).unwrap(), "https://api.vworld.kr/req/data?x=1");
        assert!(r.unwrap_url("https://api.vworld.kr/req/data").is_none());
    }

    #[test]
    fn rewraps_redirect_locations() {
        let r = relay();
        let cur = r.wrap("https://api.vworld.kr/req/image?service=image").unwrap();
        // 기관 절대 주소 → 다시 감싼다.
        assert_eq!(
            r.wrap_location(&cur, "https://map.vworld.kr/img/a.png?k=1").unwrap(),
            "http://127.0.0.1:53999/vworld/https/map.vworld.kr/img/a.png?k=1"
        );
        // 절대 경로.
        assert_eq!(
            r.wrap_location(&cur, "/req/other?x=1").unwrap(),
            "http://127.0.0.1:53999/vworld/https/api.vworld.kr/req/other?x=1"
        );
        // 상대 경로.
        assert_eq!(
            r.wrap_location(&cur, "image2").unwrap(),
            "http://127.0.0.1:53999/vworld/https/api.vworld.kr/req/image2"
        );
        // 질의만.
        assert_eq!(
            r.wrap_location(&cur, "?y=2").unwrap(),
            "http://127.0.0.1:53999/vworld/https/api.vworld.kr/req/image?y=2"
        );
        // 스킴 생략.
        assert_eq!(
            r.wrap_location(&cur, "//map.vworld.kr/z").unwrap(),
            "http://127.0.0.1:53999/vworld/https/map.vworld.kr/z"
        );
        // 이미 중계 주소인 Location 은 그대로.
        let again = r.wrap("https://api.vworld.kr/req/x").unwrap();
        assert_eq!(r.wrap_location(&cur, &again).unwrap(), again);
    }

    #[test]
    fn error_text_prefers_json_error() {
        assert_eq!(error_text(409, r#"{"error":"PES 에 키가 없어요"}"#), "PES 에 키가 없어요");
        assert_eq!(error_text(401, "토큰 오류"), "토큰 오류");
        assert_eq!(error_text(403, ""), "PES 중계 오류 (HTTP 403)");
    }
}
