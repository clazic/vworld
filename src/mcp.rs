//! `vworld-mcp`(stdio MCP 서버)의 공용 로직 — 키 선택·키 스크럽·입력 검증·도구 실행.
//!
//! MCP 전송(rmcp)과 무관한 순수 로직만 둔다(단위 테스트 대상). CLI 명령 핸들러(`cli::*`)는
//! stdout 에 직접 출력하므로 쓰지 않고, 그 아래 계층(`api::Client`·`QueryBuilder`·`NedBuilder`·
//! `normalize`·레지스트리)을 함수로 직접 호출한다. CLI 동작은 바꾸지 않는다.
//!
//! 키 규칙(공통 규약 §3):
//! - 기본 키: `VWORLD_API_KEYS`(쉼표 구분) + 선택 `VWORLD_DOMAIN`. 없으면 CLI 설정 파일(`~/.vworld/config.toml`).
//! - 요청별 키: `params._meta["kr.pes/keys"]`(문자열 배열). 있으면 그 키만 쓰고 기본 키로 넘어가지 않는다.
//! - 모든 출력은 [`scrub`] 로 키 원문을 `***<끝4자리>` 로 바꾼다.

use crate::api::normalize;
use crate::api::{Auth, Client, NedBuilder, NedKind, QueryBuilder};
use crate::concurrency::retry::RateLimited;
use crate::config::{default_config_path, Config};
use crate::{ned_registry, twod_registry};
use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 요청별 키가 담기는 `_meta` 항목 이름.
pub const META_KEYS: &str = "kr.pes/keys";
/// 요청별 VWorld `domain`(도메인 등록 키)이 담기는 `_meta` 항목 이름.
pub const META_DOMAIN: &str = "kr.pes/domain";
/// 도구 결과 텍스트 상한(바이트). 넘으면 잘라내고 안내를 붙인다.
pub const MAX_OUTPUT: usize = 200 * 1024;
/// 키가 하나도 없을 때의 안내.
pub const NO_KEY_MSG: &str = "VWorld API 키가 없어요. VWORLD_API_KEYS 환경변수(쉼표로 여러 개), \
`vworld config add-key <KEY>` 설정 파일, 또는 요청의 _meta[\"kr.pes/keys\"] 로 키를 주세요.";

/// 진행 보고 콜백: (지금까지, 전체, 설명). 알림 전송·묶기(1초)는 호출 쪽(MCP 전송 계층)이 맡는다.
pub type Progress<'a> = &'a (dyn Fn(u32, u32, &str) + Send + Sync);

/// 진행 보고가 필요 없을 때 넘기는 콜백.
pub fn no_progress(_: u32, _: u32, _: &str) {}

// ───────────────────────── 키 ─────────────────────────

/// 기본 키의 출처(진단용 — 키 값은 담지 않는다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Env,
    ConfigFile,
    None,
}

/// 쉼표 구분 키 문자열 → Auth 목록(공백·빈 항목 제거, 중복 제거).
pub fn parse_key_list(raw: &str, domain: Option<&str>) -> Vec<Auth> {
    let mut out: Vec<Auth> = Vec::new();
    for k in raw.split(',').map(str::trim).filter(|k| !k.is_empty()) {
        if !out.iter().any(|a| a.key == k) {
            out.push(Auth {
                key: k.to_string(),
                domain: domain.map(str::to_string),
            });
        }
    }
    out
}

/// `VWORLD_DOMAIN`(도메인 등록 키용 Referer/domain). 비어 있으면 None.
pub fn env_domain() -> Option<String> {
    std::env::var("VWORLD_DOMAIN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 기본 키 로드: `VWORLD_API_KEYS` > CLI 설정 파일. 둘 다 없으면 빈 목록.
pub fn load_default_auths() -> (Vec<Auth>, KeySource) {
    let domain = env_domain();
    if let Ok(raw) = std::env::var("VWORLD_API_KEYS") {
        let auths = parse_key_list(&raw, domain.as_deref());
        if !auths.is_empty() {
            return (auths, KeySource::Env);
        }
    }
    let Ok(path) = default_config_path() else {
        return (Vec::new(), KeySource::None);
    };
    match Config::load(&path) {
        Ok(cfg) if !cfg.keys.is_empty() => {
            let auths = cfg
                .keys
                .iter()
                .map(|e| Auth {
                    key: e.key.clone(),
                    domain: domain.clone().or_else(|| cfg.referer_for(e)),
                })
                .collect();
            (auths, KeySource::ConfigFile)
        }
        _ => (Vec::new(), KeySource::None),
    }
}

/// 키 한 개의 형식 검사(영숫자·`-`·`_`, 1~128자). 값은 오류 메시지에 넣지 않는다.
fn valid_key_format(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 128
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 이번 요청에 쓸 키를 고른다.
///
/// - `meta_keys` 가 있으면(요청별 키) 그 키만 쓴다 — 기본 키로 절대 넘어가지 않는다.
/// - 없으면 기본 키. 기본 키도 없으면 [`NO_KEY_MSG`].
///
/// 반환: (키 목록, 요청별 키인지).
pub fn select_auths(
    meta_keys: Option<&Value>,
    defaults: &[Auth],
    domain: Option<&str>,
) -> std::result::Result<(Vec<Auth>, bool), String> {
    match meta_keys {
        None | Some(Value::Null) => {
            if defaults.is_empty() {
                Err(NO_KEY_MSG.to_string())
            } else {
                Ok((defaults.to_vec(), false))
            }
        }
        Some(Value::Array(items)) => {
            if items.len() > 32 {
                return Err(format!("_meta[\"{META_KEYS}\"] 키는 32개 이하로 주세요"));
            }
            let mut auths: Vec<Auth> = Vec::new();
            for (i, it) in items.iter().enumerate() {
                let Some(s) = it.as_str().map(str::trim) else {
                    return Err(format!("_meta[\"{META_KEYS}\"][{i}] 는 문자열이어야 해요"));
                };
                if !valid_key_format(s) {
                    return Err(format!(
                        "_meta[\"{META_KEYS}\"][{i}] 키 형식이 올바르지 않아요(영숫자·-·_, 128자 이하)"
                    ));
                }
                if !auths.iter().any(|a| a.key == s) {
                    auths.push(Auth {
                        key: s.to_string(),
                        domain: domain.map(str::to_string),
                    });
                }
            }
            if auths.is_empty() {
                return Err(NO_KEY_MSG.to_string());
            }
            Ok((auths, true))
        }
        Some(_) => Err(format!("_meta[\"{META_KEYS}\"] 는 문자열 배열이어야 해요")),
    }
}

/// 출력 텍스트에서 키 원문을 `***<끝4자리>` 로 바꾼다(긴 키부터, URL 인코딩 형태 포함).
/// 8자 미만의 짧은 키는 끝자리도 드러내지 않고 `***` 만 남긴다.
pub fn scrub(text: &str, secrets: &[&str]) -> String {
    let mut keys: Vec<&str> = secrets.iter().copied().filter(|s| !s.is_empty()).collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    keys.dedup();
    let mut out = text.to_string();
    for k in keys {
        let chars: Vec<char> = k.chars().collect();
        let masked = if chars.len() >= 8 {
            format!("***{}", chars[chars.len() - 4..].iter().collect::<String>())
        } else {
            "***".to_string()
        };
        out = out.replace(k, &masked);
        let enc = percent_encode(k);
        if enc != k {
            out = out.replace(&enc, &masked);
        }
    }
    out
}

/// 도메인 값 검사: `http(s)://호스트[:포트][/경로]` 또는 맨 호스트, 200자 이하, 공백·제어문자 없음.
pub fn check_domain(d: &str) -> std::result::Result<(), String> {
    let bad =
        || "domain 은 http(s) URL 이나 호스트 이름이어야 해요(200자 이하, 공백 없이)".to_string();
    if d.is_empty() || d.len() > 200 || d.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad());
    }
    let rest = d
        .strip_prefix("https://")
        .or_else(|| d.strip_prefix("http://"))
        .unwrap_or(d);
    if d.contains("://") && rest.len() == d.len() {
        return Err(bad()); // http(s) 아닌 스킴
    }
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_ok = !host_port.is_empty()
        && host_port
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_:[]".contains(c))
        && !host_port.starts_with(['.', '-', ':']);
    let tail_ok = rest[host_port.len()..]
        .chars()
        .all(|c| c.is_ascii_graphic() && !"<>\"\\^`{|}".contains(c));
    // 맨 호스트는 경로 없이.
    let bare_ok = d.contains("://") || host_port.len() == rest.len();
    if host_ok && tail_ok && bare_ok {
        Ok(())
    } else {
        Err(bad())
    }
}

/// 요청별 도메인: `_meta["kr.pes/domain"]`. 없으면 None, 있는데 틀리면 오류.
pub fn select_domain(meta_domain: Option<&Value>) -> std::result::Result<Option<String>, String> {
    match meta_domain {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let s = s.trim();
            check_domain(s)?;
            Ok(Some(s.to_string()))
        }
        Some(_) => Err(format!("_meta[\"{META_DOMAIN}\"] 는 문자열이어야 해요")),
    }
}

/// 이번 호출의 키 목록에 요청별 도메인을 덮어쓴다(호출마다 따로 만든 목록이라 다른 호출과 섞이지 않음).
pub fn with_domain(mut auths: Vec<Auth>, domain: Option<&str>) -> Vec<Auth> {
    if let Some(d) = domain {
        for a in &mut auths {
            a.domain = Some(d.to_string());
        }
    }
    auths
}

/// 쿼리 문자열식 퍼센트 인코딩(영숫자·`-_.~` 외 전부) — 스크럽 대조용.
fn percent_encode(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// 결과 텍스트가 [`MAX_OUTPUT`] 를 넘으면 문자 경계에서 잘라내고 안내를 붙인다.
pub fn cap_output(text: String) -> String {
    if text.len() <= MAX_OUTPUT {
        return text;
    }
    let mut cut = MAX_OUTPUT;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!(
        "{}\n…[잘림: 결과가 {}바이트로 너무 커요. 범위를 좁히세요(size·bbox·maxFeatures·필터)]",
        &text[..cut],
        text.len()
    )
}

// ───────────────────────── 키 풀(요청 단위) ─────────────────────────

/// 키별 429 쿨다운 만료 시각. 키 원문 대신 해시를 쓴다(메모리에도 키 사본을 늘리지 않음).
static COOLDOWN: Mutex<Option<HashMap<u64, Instant>>> = Mutex::new(None);
/// 라운드로빈 시작점(호출마다 다음 키부터).
static ROTOR: AtomicUsize = AtomicUsize::new(0);

fn key_hash(k: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    k.hash(&mut h);
    h.finish()
}

fn cooling(k: &str) -> bool {
    let now = Instant::now();
    let g = COOLDOWN.lock().unwrap();
    g.as_ref()
        .and_then(|m| m.get(&key_hash(k)))
        .is_some_and(|until| *until > now)
}

fn set_cooldown(k: &str, d: Duration) {
    let mut g = COOLDOWN.lock().unwrap();
    g.get_or_insert_with(HashMap::new)
        .insert(key_hash(k), Instant::now() + d);
}

/// 주어진 키 목록 안에서만 키를 돌려 가며 `f` 를 실행한다(kosis 동시성 패턴 차용).
///
/// - 시작 키는 라운드로빈, 429 쿨다운 중인 키는 뒤로 미룬다.
/// - 429 소진(`RateLimited`)이면 그 키를 쿨다운하고 **같은 목록의** 다음 키로 넘어간다.
/// - 그 밖의 오류는 바로 돌려준다. 목록 밖의 키(기본 키 등)는 절대 쓰지 않는다.
pub async fn with_key<T, F, Fut>(auths: &[Auth], f: F) -> Result<T>
where
    F: Fn(Auth) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    if auths.is_empty() {
        return Err(anyhow!(NO_KEY_MSG));
    }
    let n = auths.len();
    let start = ROTOR.fetch_add(1, Ordering::Relaxed) % n;
    let mut order: Vec<&Auth> = (0..n).map(|i| &auths[(start + i) % n]).collect();
    order.sort_by_key(|a| cooling(&a.key)); // 안정 정렬: 쿨다운 아닌 키 먼저.
    let mut last: Option<anyhow::Error> = None;
    for a in order {
        match f(a.clone()).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if let Some(rl) = e.chain().find_map(|c| c.downcast_ref::<RateLimited>()) {
                    set_cooldown(&a.key, rl.cooldown());
                    last = Some(e);
                    continue;
                }
                return Err(e);
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("모든 키가 요청 과다(429) 상태예요")))
}

// ───────────────────────── 입력 검증 ─────────────────────────

type VResult = std::result::Result<(), String>;

fn non_empty(name: &str, s: &str, max_chars: usize) -> VResult {
    let t = s.trim();
    if t.is_empty() {
        return Err(format!("{name} 이(가) 비어 있어요"));
    }
    if t.chars().count() > max_chars {
        return Err(format!("{name} 는 {max_chars}자 이하로 주세요"));
    }
    if t.chars().any(|c| c.is_control()) {
        return Err(format!("{name} 에 제어문자를 넣을 수 없어요"));
    }
    Ok(())
}

/// `EPSG:<4~6자리>`.
pub fn check_crs(name: &str, s: &str) -> VResult {
    let ok = s
        .strip_prefix("EPSG:")
        .or_else(|| s.strip_prefix("epsg:"))
        .is_some_and(|d| (4..=6).contains(&d.len()) && d.chars().all(|c| c.is_ascii_digit()));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{name} 형식은 EPSG:4326 처럼 'EPSG:숫자' 여야 해요"
        ))
    }
}

/// 쉼표 구분 숫자 4개(선택적으로 5번째 EPSG 코드).
pub fn check_bbox(name: &str, s: &str) -> VResult {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    let nums = parts
        .iter()
        .take(4)
        .all(|p| p.parse::<f64>().is_ok_and(f64::is_finite));
    let ok = match parts.len() {
        4 => nums,
        5 => nums && check_crs(name, parts[4]).is_ok(),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{name} 는 'minx,miny,maxx,maxy' 숫자 4개여야 해요(선택: 5번째 EPSG:코드)"
        ))
    }
}

/// PNU(필지고유번호) — 숫자 8~19자리.
pub fn check_pnu(s: &str) -> VResult {
    if (8..=19).contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err("pnu 는 숫자 8~19자리여야 해요(법정동 8자리 이상)".into())
    }
}

/// 2D 데이터 공간 필터: `POINT(x y)` / `BOX(minx,miny,maxx,maxy)` / `POLYGON((...))` / `LINESTRING(...)`.
/// EPSG 접미사는 서버가 INVALID_RANGE 로 거부하므로 미리 막는다.
pub fn check_geom_filter(s: &str) -> VResult {
    non_empty("geom_filter", s, 4000)?;
    let up = s.trim().to_ascii_uppercase();
    let head_ok = ["POINT(", "BOX(", "POLYGON(", "LINESTRING(", "MULTIPOLYGON("]
        .iter()
        .any(|h| up.starts_with(h));
    if !head_ok || !up.ends_with(')') {
        return Err("geom_filter 는 POINT(x y) · BOX(minx,miny,maxx,maxy) · POLYGON((x y,...)) 형식이어야 해요".into());
    }
    if up.contains("EPSG") {
        return Err("geom_filter 에 EPSG 접미사를 붙이면 서버가 INVALID_RANGE 로 거부해요(좌표만, EPSG:4326 해석)".into());
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " .,()-+".contains(c))
    {
        return Err("geom_filter 에 허용되지 않는 문자가 있어요".into());
    }
    Ok(())
}

fn check_ident_list(name: &str, s: &str, max: usize) -> VResult {
    if s.is_empty()
        || s.len() > max
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ',')
    {
        return Err(format!(
            "{name} 는 영숫자·_ 를 쉼표로 이은 {max}자 이하여야 해요"
        ));
    }
    Ok(())
}

fn check_range(name: &str, v: u32, min: u32, max: u32) -> VResult {
    if (min..=max).contains(&v) {
        Ok(())
    } else {
        Err(format!("{name} 는 {min}~{max} 범위여야 해요"))
    }
}

fn check_lonlat(x: f64, y: f64) -> VResult {
    if !(x.is_finite() && y.is_finite())
        || !(-180.0..=180.0).contains(&x)
        || !(-90.0..=90.0).contains(&y)
    {
        return Err(
            "x(경도)·y(위도)가 EPSG:4326 범위를 벗어났어요(한국은 x≈124~132, y≈33~39)".into(),
        );
    }
    Ok(())
}

// ───────────────────────── 공용 호출 ─────────────────────────

/// VWorld 텍스트 응답을 JSON 으로 정규화하고 본문 에러를 검사한다. "결과 없음"은 `Null`.
fn normalize_body(body: &str) -> Result<Value> {
    let v = normalize::parse_to_json(body)?;
    match normalize::check_body_error(&v) {
        Ok(()) => Ok(v),
        Err(e) if e.empty_ok => Ok(Value::Null),
        Err(e) => Err(anyhow!("{e}")),
    }
}

async fn fetch_json(
    client: &Client,
    auths: &[Auth],
    url: String,
    params: Vec<(String, String)>,
) -> Result<Value> {
    with_key(auths, |a| {
        let url = url.clone();
        let params = params.clone();
        async move {
            let body = client.get_text(&url, params, &a).await?;
            normalize_body(&body)
        }
    })
    .await
}

// ───────────────────────── 도구: 지오코딩 ─────────────────────────

/// 주소 → 좌표(`/req/address` GetCoord). `addr_type` auto 면 도로명→지번 순 폴백(CLI 와 같음).
pub async fn geocode(
    client: &Client,
    auths: &[Auth],
    address: &str,
    addr_type: &str,
    crs: &str,
) -> Result<Value> {
    non_empty("address", address, 200).map_err(|e| anyhow!(e))?;
    check_crs("crs", crs).map_err(|e| anyhow!(e))?;
    let types: Vec<&str> = match addr_type.to_ascii_uppercase().as_str() {
        "AUTO" | "" => vec!["ROAD", "PARCEL"],
        "ROAD" => vec!["ROAD"],
        "PARCEL" => vec!["PARCEL"],
        _ => return Err(anyhow!("type 은 auto · ROAD · PARCEL 중 하나예요")),
    };
    for ty in &types {
        let (url, params) = QueryBuilder::new("address", "GetCoord")
            .format("json")
            .set("type", ty)
            .set("crs", crs)
            .set("address", address.trim())
            .build();
        let v = fetch_json(client, auths, url, params).await?;
        if v["response"]["result"]["point"]["x"].is_string() {
            return Ok(json!({
                "mode": "forward",
                "type": ty,
                "point": v["response"]["result"]["point"],
                "refined": v["response"]["refined"],
                "result": v,
            }));
        }
    }
    Ok(
        json!({"mode": "forward", "result": Value::Null, "note": format!("결과 없음: {} (도로명·지번 모두 미발견)", address.trim())}),
    )
}

/// 좌표 → 주소(`/req/address` GetAddress). `addr_type`: BOTH(기본) · ROAD · PARCEL.
pub async fn reverse_geocode(
    client: &Client,
    auths: &[Auth],
    x: f64,
    y: f64,
    addr_type: &str,
    crs: &str,
) -> Result<Value> {
    check_crs("crs", crs).map_err(|e| anyhow!(e))?;
    if crs.eq_ignore_ascii_case("EPSG:4326") {
        check_lonlat(x, y).map_err(|e| anyhow!(e))?;
    } else if !(x.is_finite() && y.is_finite()) {
        return Err(anyhow!("x·y 는 유한한 숫자여야 해요"));
    }
    let ty = addr_type.to_ascii_uppercase();
    if !["BOTH", "ROAD", "PARCEL"].contains(&ty.as_str()) {
        return Err(anyhow!("type 은 BOTH · ROAD · PARCEL 중 하나예요"));
    }
    let (url, params) = QueryBuilder::new("address", "GetAddress")
        .format("json")
        .set("type", &ty)
        .set("crs", crs)
        .set("point", &format!("{x},{y}"))
        .build();
    let v = fetch_json(client, auths, url, params).await?;
    if v.is_null() || v["response"]["status"].as_str() == Some("NOT_FOUND") {
        return Ok(
            json!({"mode": "reverse", "result": Value::Null, "note": "결과 없음(바다·경계 밖일 수 있어요)"}),
        );
    }
    Ok(json!({"mode": "reverse", "type": ty, "result": v}))
}

// ───────────────────────── 도구: 검색 ─────────────────────────

/// 검색 인자.
#[derive(Debug, Clone, Default)]
pub struct SearchInput {
    pub query: String,
    pub kind: String,
    pub category: Option<String>,
    pub size: Option<u32>,
    pub page: Option<u32>,
    pub bbox: Option<String>,
    pub crs: String,
}

/// 검색 인자 검증(ADDRESS/DISTRICT 의 category 필수 규칙 포함).
pub fn validate_search(a: &SearchInput) -> VResult {
    non_empty("query", &a.query, 100)?;
    check_crs("crs", &a.crs)?;
    let kind = a.kind.to_ascii_uppercase();
    let cat = a.category.as_deref().map(str::to_ascii_uppercase);
    match kind.as_str() {
        "PLACE" | "ROAD" => {}
        "ADDRESS" => match cat.as_deref() {
            Some("ROAD") | Some("PARCEL") => {}
            _ => return Err("type=ADDRESS 는 category 가 ROAD 또는 PARCEL 이어야 해요(필수)".into()),
        },
        "DISTRICT" => match cat.as_deref() {
            Some("L1") | Some("L2") | Some("L3") | Some("L4") => {}
            _ => return Err("type=DISTRICT 는 category 가 L1(시도)·L2(시군구)·L3(읍면동)·L4(리) 중 하나여야 해요(필수)".into()),
        },
        _ => return Err("type 은 PLACE · ADDRESS · DISTRICT · ROAD 중 하나예요".into()),
    }
    if let Some(c) = &a.category {
        if c.len() > 20 || !c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            return Err("category 형식이 올바르지 않아요".into());
        }
    }
    if let Some(s) = a.size {
        check_range("size", s, 1, 100)?;
    }
    if let Some(p) = a.page {
        check_range("page", p, 1, 1000)?;
    }
    if let Some(b) = &a.bbox {
        check_bbox("bbox", b)?;
    }
    Ok(())
}

/// `/req/search`.
pub async fn search(client: &Client, auths: &[Auth], a: &SearchInput) -> Result<Value> {
    validate_search(a).map_err(|e| anyhow!(e))?;
    let size = a.size.map(|s| s.to_string());
    let page = a.page.map(|p| p.to_string());
    let kind = a.kind.to_ascii_uppercase();
    let cat = a.category.as_deref().map(str::to_ascii_uppercase);
    let (url, params) = QueryBuilder::new("search", "search")
        .format("json")
        .set("query", a.query.trim())
        .set("type", &kind)
        .set("crs", &a.crs)
        .opt("category", cat.as_deref())
        .opt("size", size.as_deref())
        .opt("page", page.as_deref())
        .opt("bbox", a.bbox.as_deref())
        .build();
    let v = fetch_json(client, auths, url, params).await?;
    if v.is_null() {
        return Ok(json!({"result": Value::Null, "note": "결과 없음"}));
    }
    Ok(json!({"result": v}))
}

// ───────────────────────── 도구: 2D 데이터 ─────────────────────────

/// 2D 데이터 레이어 목록(오프라인 레지스트리, 키 불요).
pub fn data_layers(search: Option<&str>, cat: Option<&str>, geom: Option<&str>) -> Result<Value> {
    if let Some(s) = search {
        non_empty("search", s, 50).map_err(|e| anyhow!(e))?;
    }
    if let Some(c) = cat {
        non_empty("cat", c, 50).map_err(|e| anyhow!(e))?;
    }
    let geom = match geom.map(str::to_ascii_lowercase).as_deref() {
        None => None,
        Some("polygon") => Some("Polygon"),
        Some("line") => Some("Line"),
        Some("point") => Some("Point"),
        Some(_) => return Err(anyhow!("geom 은 polygon · line · point 중 하나예요")),
    };
    let q = search.map(|s| s.trim().to_lowercase());
    let layers: Vec<Value> = twod_registry::all()
        .iter()
        .filter(|l| {
            q.as_ref().is_none_or(|q| {
                l.data_id.to_lowercase().contains(q) || l.name.to_lowercase().contains(q) || l.cat.to_lowercase().contains(q)
            }) && cat.is_none_or(|c| l.cat.eq_ignore_ascii_case(c.trim()))
                && geom.is_none_or(|g| l.geom == g)
        })
        .map(|l| json!({"data_id": l.data_id, "name": l.name, "cat": l.cat, "geom": l.geom, "attr_count": l.attrs.len()}))
        .collect();
    Ok(json!({"count": layers.len(), "layers": layers}))
}

fn check_data_id(id: &str) -> Result<&'static twod_registry::TwodLayer> {
    if id.is_empty() || id.len() > 40 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(anyhow!(
            "data_id 는 영숫자·_ 40자 이하예요(예: LP_PA_CBND_BUBUN)"
        ));
    }
    twod_registry::find(id)
        .ok_or_else(|| anyhow!("알 수 없는 data_id: {id} (vworld_data_layers 로 목록 확인)"))
}

/// 2D 데이터 레이어 하나의 속성표(오프라인, 키 불요).
pub fn data_describe(data_id: &str) -> Result<Value> {
    let l = check_data_id(data_id)?;
    let single: Vec<&str> = l
        .attrs
        .iter()
        .filter(|a| a.single_search)
        .map(|a| a.name)
        .collect();
    Ok(json!({
        "data_id": l.data_id, "svc_ide": l.svc_ide, "name": l.name, "cat": l.cat, "geom": l.geom,
        "attrs": l.attrs, "single_search_keys": single,
    }))
}

/// 2D 데이터 조회 인자.
#[derive(Debug, Clone, Default)]
pub struct DataInput {
    pub data_id: String,
    pub geom_filter: Option<String>,
    pub attr_filter: Option<String>,
    pub emd_cd: Option<String>,
    pub columns: Option<String>,
    pub size: Option<u32>,
    pub page: Option<u32>,
    /// 여러 쪽 받기(1~10, 기본 1). page 부터 이어서 받아 features 를 합친다.
    pub pages: Option<u32>,
    pub crs: String,
}

/// 2D 데이터 조회 인자 검증.
pub fn validate_data(a: &DataInput) -> VResult {
    check_data_id(&a.data_id).map_err(|e| e.to_string())?;
    check_crs("crs", &a.crs)?;
    if a.geom_filter.is_none() && a.attr_filter.is_none() && a.emd_cd.is_none() {
        return Err("geom_filter · attr_filter · emd_cd 중 하나는 있어야 해요(보통 geom_filter 나 attr_filter)".into());
    }
    if let Some(g) = &a.geom_filter {
        check_geom_filter(g)?;
    }
    if let Some(f) = &a.attr_filter {
        non_empty("attr_filter", f, 500)?;
        if f.split('|').any(|c| c.splitn(3, ':').count() != 3) {
            return Err("attr_filter 는 '속성:연산자:값' 을 | 로 이은 형식이에요(예: pnu:=:1114010300100310000)".into());
        }
    }
    if let Some(e) = &a.emd_cd {
        if !(8..=10).contains(&e.len()) || !e.chars().all(|c| c.is_ascii_digit()) {
            return Err("emd_cd 는 숫자 8~10자리예요".into());
        }
    }
    if let Some(c) = &a.columns {
        check_ident_list("columns", c, 500)?;
    }
    if let Some(s) = a.size {
        check_range("size", s, 1, 1000)?;
    }
    if let Some(p) = a.page {
        check_range("page", p, 1, 10000)?;
    }
    if let Some(p) = a.pages {
        check_range("pages", p, 1, 10)?;
    }
    Ok(())
}

/// `/req/data` GetFeature. size 미지정 시 100(상한 1000).
pub async fn data(
    client: &Client,
    auths: &[Auth],
    a: &DataInput,
    progress: Progress<'_>,
) -> Result<Value> {
    validate_data(a).map_err(|e| anyhow!(e))?;
    let size = a.size.unwrap_or(100).to_string();
    let id = check_data_id(&a.data_id)?.data_id;
    let first = a.page.unwrap_or(1);
    let want = a.pages.unwrap_or(1);
    let mut total = want;
    progress(0, total, "2D 데이터 조회 시작");
    let mut merged: Option<Value> = None;
    let mut fetched = 0u32;
    for i in 0..want {
        let page = (first + i).to_string();
        let (url, params) = QueryBuilder::new("data", "GetFeature")
            .format("json")
            .set("data", id)
            .set("crs", &a.crs)
            .opt("geomFilter", a.geom_filter.as_deref().map(str::trim))
            .opt("attrFilter", a.attr_filter.as_deref())
            .opt("emdCd", a.emd_cd.as_deref())
            .opt("columns", a.columns.as_deref())
            .set("size", &size)
            .opt("page", (a.page.is_some() || i > 0).then_some(page.as_str()))
            .build();
        let v = fetch_json(client, auths, url, params).await?;
        if v.is_null() {
            break;
        }
        fetched += 1;
        let server_pages = v["response"]["page"]["total"]
            .as_str()
            .and_then(|t| t.parse::<u32>().ok())
            .unwrap_or(1);
        total = want
            .min(server_pages.saturating_sub(first) + 1)
            .max(fetched);
        match merged.as_mut() {
            None => merged = Some(v),
            Some(m) => {
                let more = v["response"]["result"]["featureCollection"]["features"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if let Some(arr) =
                    m["response"]["result"]["featureCollection"]["features"].as_array_mut()
                {
                    arr.extend(more);
                }
            }
        }
        progress(
            fetched,
            total,
            &format!("{}/{server_pages}쪽 받음", first + i),
        );
        if first + i >= server_pages {
            break;
        }
    }
    progress(total, total, "2D 데이터 조회 완료");
    match merged {
        None => Ok(json!({"data_id": id, "result": Value::Null, "note": "결과 없음"})),
        Some(v) => Ok(json!({"data_id": id, "pages_fetched": fetched, "result": v})),
    }
}

// ───────────────────────── 도구: 용도지역(토지이용계획) ─────────────────────────

/// 한 지점의 필지와 토지이용계획(용도지역·지구 등).
///
/// 표준 체인(SKILL.md·LEARNINGS.md): (주소면 지오코딩) → 연속지적도 `LP_PA_CBND_BUBUN` POINT 조회로 PNU →
/// NED `getLandUseAttr`(pnu).
pub async fn zoning(
    client: &Client,
    auths: &[Auth],
    address: Option<&str>,
    x: Option<f64>,
    y: Option<f64>,
    progress: Progress<'_>,
) -> Result<Value> {
    let steps = if address.is_some() { 3 } else { 2 };
    progress(0, steps, "용도지역 조회 시작");
    let (x, y, geocoded) = match (address, x, y) {
        (Some(addr), None, None) => {
            let g = geocode(client, auths, addr, "auto", "EPSG:4326").await?;
            progress(1, steps, "주소 → 좌표 완료");
            let px = g["point"]["x"].as_str().and_then(|s| s.parse::<f64>().ok());
            let py = g["point"]["y"].as_str().and_then(|s| s.parse::<f64>().ok());
            match (px, py) {
                (Some(px), Some(py)) => (
                    px,
                    py,
                    Some(
                        json!({"address": addr.trim(), "type": g["type"], "refined": g["refined"]}),
                    ),
                ),
                _ => {
                    return Ok(
                        json!({"result": Value::Null, "note": format!("주소를 좌표로 바꾸지 못했어요: {}", addr.trim())}),
                    )
                }
            }
        }
        (None, Some(x), Some(y)) => (x, y, None),
        _ => {
            return Err(anyhow!(
                "address 하나, 또는 x·y(EPSG:4326 경도·위도) 둘 중 한 가지만 주세요"
            ))
        }
    };
    check_lonlat(x, y).map_err(|e| anyhow!(e))?;

    let parcel = data(
        client,
        auths,
        &DataInput {
            data_id: "LP_PA_CBND_BUBUN".into(),
            geom_filter: Some(format!("POINT({x} {y})")),
            size: Some(1),
            crs: "EPSG:4326".into(),
            ..Default::default()
        },
        &no_progress,
    )
    .await?;
    progress(steps - 1, steps, "좌표 → 필지(PNU) 완료");
    let props =
        &parcel["result"]["response"]["result"]["featureCollection"]["features"][0]["properties"];
    let Some(pnu) = props["pnu"].as_str().map(str::to_string) else {
        return Ok(
            json!({"point": {"x": x, "y": y}, "geocoded": geocoded, "parcel": Value::Null,
            "note": "이 지점에서 필지를 찾지 못했어요(도로·하천·바다일 수 있어요)"}),
        );
    };
    let parcel_props = json!({"pnu": pnu, "addr": props["addr"], "jibun": props["jibun"]});

    let (url, params) = NedBuilder::new(NedKind::Data, "getLandUseAttr")
        .set("pnu", &pnu)
        .set("numOfRows", "100")
        .build();
    let land = fetch_json(client, auths, url, params).await?;
    progress(steps, steps, "토지이용계획 조회 완료");
    let mut zones: Vec<Value> = Vec::new();
    collect_land_use(&land, &mut zones);
    Ok(json!({
        "point": {"x": x, "y": y},
        "geocoded": geocoded,
        "parcel": parcel_props,
        "land_use_summary": zones,
        "land_use": land,
    }))
}

/// getLandUseAttr 응답에서 `prposAreaDstrcCodeNm`(용도지역지구명)·`cnflcAtNm`(저촉여부)을 모은다.
fn collect_land_use(v: &Value, out: &mut Vec<Value>) {
    match v {
        Value::Object(m) => {
            if let Some(nm) = m.get("prposAreaDstrcCodeNm").and_then(Value::as_str) {
                out.push(json!({"name": nm, "conflict": m.get("cnflcAtNm").cloned().unwrap_or(Value::Null)}));
                return;
            }
            m.values().for_each(|c| collect_land_use(c, out));
        }
        Value::Array(a) => a.iter().for_each(|c| collect_land_use(c, out)),
        _ => {}
    }
}

// ───────────────────────── 도구: 국가중점데이터(NED) ─────────────────────────

/// NED 오퍼레이션 목록(오프라인). `operation` 을 주면 그 오퍼레이션의 요청변수까지.
pub fn ned_list(
    kind: Option<&str>,
    search: Option<&str>,
    operation: Option<&str>,
) -> Result<Value> {
    if let Some(op) = operation {
        let o = ned_registry::find(op.trim())
            .ok_or_else(|| anyhow!("알 수 없는 NED 오퍼레이션: {op}"))?;
        let params: Vec<Value> = o
            .params
            .iter()
            .filter(|p| !["key", "domain"].contains(&p.name.to_ascii_lowercase().as_str()))
            .map(|p| json!({"name": p.name, "required": p.required, "example": p.default, "desc": p.desc}))
            .collect();
        return Ok(
            json!({"operation": o.endpoint_op, "kind": o.kind, "name": o.name, "cat1": o.cat1, "cat2": o.cat2,
            "callable": o.kind != "wms", "params": params}),
        );
    }
    let kind = kind.map(str::to_ascii_lowercase);
    if let Some(k) = &kind {
        if !["wms", "wfs", "data"].contains(&k.as_str()) {
            return Err(anyhow!("kind 는 data · wfs · wms 중 하나예요"));
        }
    }
    if let Some(s) = search {
        non_empty("search", s, 50).map_err(|e| anyhow!(e))?;
    }
    let q = search.map(|s| s.trim().to_lowercase());
    let ops: Vec<Value> = ned_registry::all()
        .iter()
        .filter(|o| kind.as_deref().is_none_or(|k| o.kind == k))
        .filter(|o| {
            q.as_ref().is_none_or(|q| {
                o.endpoint_op.to_lowercase().contains(q)
                    || o.name.to_lowercase().contains(q)
                    || o.cat2.to_lowercase().contains(q)
            })
        })
        .map(
            |o| json!({"operation": o.endpoint_op, "kind": o.kind, "name": o.name, "cat2": o.cat2}),
        )
        .collect();
    Ok(json!({"count": ops.len(), "operations": ops,
        "note": "wms 계열은 이미지라 이 서버에서 부를 수 없어요. data(속성)·wfs(도형) 계열을 쓰세요."}))
}

/// NED 조회 인자.
#[derive(Debug, Clone, Default)]
pub struct NedInput {
    pub operation: String,
    pub pnu: Option<String>,
    pub bbox: Option<String>,
    pub crs: Option<String>,
    pub params: Map<String, Value>,
}

const NED_RESERVED: &[&str] = &["key", "domain", "apikey", "servicekey", "format", "output"];

/// 검증된 NED 호출: (오퍼레이션, 추가 요청변수).
pub type NedCall = (&'static ned_registry::NedOp, Vec<(String, String)>);

pub fn validate_ned(a: &NedInput) -> std::result::Result<NedCall, String> {
    let op_name = a.operation.trim();
    if op_name.is_empty()
        || op_name.len() > 60
        || !op_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err("operation 은 영숫자 60자 이하예요(예: getLandUseAttr)".into());
    }
    let op = ned_registry::find(op_name)
        .ok_or_else(|| format!("알 수 없는 NED 오퍼레이션: {op_name} (vworld_ned_list 로 확인)"))?;
    if op.kind == "wms" {
        return Err(format!("{op_name} 은 WMS(이미지) 계열이라 지원하지 않아요. 같은 데이터의 WFS·속성(data) 계열을 쓰세요."));
    }
    if let Some(p) = &a.pnu {
        check_pnu(p)?;
    }
    if let Some(b) = &a.bbox {
        check_bbox("bbox", b)?;
    }
    if let Some(c) = &a.crs {
        check_crs("crs", c)?;
    }
    if a.params.len() > 20 {
        return Err("params 는 20개 이하로 주세요".into());
    }
    let mut extra: Vec<(String, String)> = Vec::new();
    for (k, v) in &a.params {
        if k.is_empty()
            || k.len() > 40
            || !k.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(format!(
                "params 이름 '{k}' 형식이 올바르지 않아요(영문으로 시작하는 영숫자)"
            ));
        }
        if NED_RESERVED.contains(&k.to_ascii_lowercase().as_str()) {
            return Err(format!("params 로 '{k}' 는 줄 수 없어요(서버가 정함)"));
        }
        let s = match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => return Err(format!("params.{k} 값은 문자열·숫자여야 해요")),
        };
        non_empty(&format!("params.{k}"), &s, 500)?;
        extra.push((k.clone(), s));
    }
    // 레지스트리 필수 요청변수 확인(인증 변수 제외, 예시값은 충족 근거가 아님 — CLI 와 같은 규칙).
    let has = |n: &str| {
        (n == "pnu" && a.pnu.is_some())
            || (n == "bbox" && a.bbox.is_some())
            || (op.kind == "wfs" && n == "output")
            || (op.kind == "data" && n == "format")
            || extra.iter().any(|(k, _)| k.eq_ignore_ascii_case(n))
    };
    let missing: Vec<&str> = op
        .params
        .iter()
        .filter(|p| p.required)
        .map(|p| p.name)
        .filter(|n| {
            !["key", "domain", "apikey", "servicekey"].contains(&n.to_ascii_lowercase().as_str())
        })
        .filter(|n| !has(&n.to_ascii_lowercase()))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "필수 요청변수가 빠졌어요: {} (vworld_ned_list 에 operation 을 주면 변수 목록을 볼 수 있어요)",
            missing.join(", ")
        ));
    }
    Ok((op, extra))
}

/// NED data/WFS 단건 조회. WFS 는 좌표계 미지정 시 EPSG:4326(srsName), data 는 CLI 와 같이 EPSG:5187(crs).
pub async fn ned(
    client: &Client,
    auths: &[Auth],
    a: &NedInput,
    progress: Progress<'_>,
) -> Result<Value> {
    let (op, extra) = validate_ned(a).map_err(|e| anyhow!(e))?;
    progress(0, 1, &format!("NED {} 조회 시작", op.endpoint_op));
    let kind = if op.kind == "wfs" {
        NedKind::Wfs
    } else {
        NedKind::Data
    };
    let has = |k: &str| extra.iter().any(|(x, _)| x.eq_ignore_ascii_case(k));
    let mut b = NedBuilder::new(kind, op.endpoint_op)
        .opt("pnu", a.pnu.as_deref())
        .opt("bbox", a.bbox.as_deref());
    if kind == NedKind::Wfs {
        if !has("srsName") {
            b = b.set("srsName", a.crs.as_deref().unwrap_or("EPSG:4326"));
        }
    } else if !has("crs") {
        b = b.set("crs", a.crs.as_deref().unwrap_or("EPSG:5187"));
    }
    for (k, v) in &extra {
        b = b.set(k, v);
    }
    let (url, params) = b.build();
    let v = fetch_json(client, auths, url, params).await?;
    progress(1, 1, &format!("NED {} 조회 완료", op.endpoint_op));
    if v.is_null() {
        return Ok(
            json!({"operation": op.endpoint_op, "kind": op.kind, "result": Value::Null, "note": "결과 없음"}),
        );
    }
    Ok(json!({"operation": op.endpoint_op, "kind": op.kind, "result": v}))
}

// ───────────────────────── 큰 결과용 목록 뽑기 ─────────────────────────

/// `vworld_data` 결과의 지형 목록(GeoJSON Feature) — (항목, 도형 여부).
pub fn data_items(out: &Value) -> Option<(Vec<Value>, bool)> {
    out.pointer("/result/response/result/featureCollection/features")
        .and_then(Value::as_array)
        .map(|a| (a.clone(), true))
}

/// `vworld_data` 요약(서버 전체 건수·쪽 수 등).
pub fn data_info(out: &Value) -> Value {
    json!({
        "data_id": out["data_id"],
        "pages_fetched": out["pages_fetched"],
        "server_total": out.pointer("/result/response/record/total").cloned().unwrap_or(Value::Null),
        "server_pages": out.pointer("/result/response/page/total").cloned().unwrap_or(Value::Null),
    })
}

fn find_field(v: &Value) -> Option<Vec<Value>> {
    match v {
        Value::Object(m) => {
            if let Some(f) = m.get("field") {
                return match f {
                    Value::Array(a) => Some(a.clone()),
                    Value::Object(_) => Some(vec![f.clone()]),
                    _ => None,
                };
            }
            m.values().find_map(find_field)
        }
        Value::Array(a) => a.iter().find_map(find_field),
        _ => None,
    }
}

/// `vworld_ned` 결과의 목록: WFS 는 지형(Feature), data 계열은 `field` 행.
pub fn ned_items(out: &Value) -> Option<(Vec<Value>, bool)> {
    if out["kind"] == "wfs" {
        return out
            .pointer("/result/features")
            .and_then(Value::as_array)
            .map(|a| (a.clone(), true));
    }
    find_field(&out["result"]).map(|rows| (rows, false))
}

/// `vworld_ned` 요약.
pub fn ned_info(out: &Value) -> Value {
    fn find_num(v: &Value, key: &str) -> Option<Value> {
        match v {
            Value::Object(m) => m
                .get(key)
                .cloned()
                .or_else(|| m.values().find_map(|c| find_num(c, key))),
            _ => None,
        }
    }
    json!({
        "operation": out["operation"],
        "kind": out["kind"],
        "server_total": find_num(&out["result"], "totalCount")
            .or_else(|| find_num(&out["result"], "totalFeatures"))
            .unwrap_or(Value::Null),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const K1: &str = "ABCD1234-AB69-334B-A205-D4FAFC00WXYZ";
    const K2: &str = "EEEE5678-0000-1111-2222-333344445555";

    fn auth(k: &str) -> Auth {
        Auth {
            key: k.into(),
            domain: None,
        }
    }

    #[test]
    fn scrub_replaces_keys_everywhere() {
        let text = format!(
            "HTTP 500: https://api.vworld.kr/req/address?key={K1}&x=1 / apiKey={K2} / again {K1}"
        );
        let out = scrub(&text, &[K1, K2]);
        assert!(!out.contains(K1) && !out.contains(K2), "{out}");
        assert!(out.contains("key=***WXYZ"));
        assert!(out.contains("apiKey=***5555"));
        assert_eq!(out.matches("***WXYZ").count(), 2);
    }

    #[test]
    fn scrub_handles_encoded_short_and_overlapping_keys() {
        // URL 인코딩된 형태도 지운다.
        let k = "AB+CD/EF==1234";
        let out = scrub("x=AB%2BCD%2FEF%3D%3D1234", &[k]);
        assert_eq!(out, "x=***1234");
        // 짧은 키는 끝자리도 숨긴다.
        assert_eq!(scrub("k=abc12", &["abc12"]), "k=***");
        // 한 키가 다른 키를 포함해도 긴 것부터 지워 원문이 남지 않는다.
        let out = scrub("LONGKEY123456 and KEY12345", &["KEY12345", "LONGKEY123456"]);
        assert!(
            !out.contains("KEY12345") && !out.contains("LONGKEY123456"),
            "{out}"
        );
        // 빈 키는 무시.
        assert_eq!(scrub("hello", &[""]), "hello");
    }

    #[test]
    fn request_keys_never_fall_back_to_defaults() {
        let defaults = vec![auth(K1)];
        let meta = json!([K2]);
        let (auths, per_req) = select_auths(Some(&meta), &defaults, None).unwrap();
        assert!(per_req);
        assert_eq!(auths.len(), 1);
        assert_eq!(auths[0].key, K2);
        assert!(!auths.iter().any(|a| a.key == K1));
        // 빈 배열이면 기본 키로 넘어가지 않고 오류.
        let err = select_auths(Some(&json!([])), &defaults, None).unwrap_err();
        assert!(err.contains("키가 없어요"));
    }

    #[test]
    fn default_keys_and_missing_keys() {
        let (a, per_req) = select_auths(None, &[auth(K1)], None).unwrap();
        assert!(!per_req);
        assert_eq!(a[0].key, K1);
        assert!(select_auths(None, &[], None)
            .unwrap_err()
            .contains("키가 없어요"));
    }

    #[test]
    fn bad_meta_keys_rejected_without_echoing_value() {
        let err = select_auths(Some(&json!("notarray")), &[], None).unwrap_err();
        assert!(err.contains("문자열 배열"));
        let err = select_auths(Some(&json!(["bad key with space"])), &[], None).unwrap_err();
        assert!(!err.contains("bad key"), "{err}");
        assert!(select_auths(Some(&json!([1])), &[], None).is_err());
    }

    #[test]
    fn domain_validation_and_override() {
        for ok in [
            "https://pes.uri-jylee.workers.dev",
            "http://localhost:8787",
            "pes.example.kr",
            "https://a.kr/path?x=1",
        ] {
            assert!(check_domain(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "has space.kr",
            "ftp://x.kr",
            "https://",
            "a.kr/path",
            &"a".repeat(201),
            "https://x.kr/\n",
            "-x.kr",
        ] {
            assert!(check_domain(bad).is_err(), "{bad:?}");
        }
        assert_eq!(select_domain(None).unwrap(), None);
        assert_eq!(
            select_domain(Some(&json!(" https://d.kr ")))
                .unwrap()
                .as_deref(),
            Some("https://d.kr")
        );
        assert!(select_domain(Some(&json!(5))).is_err());
        assert!(select_domain(Some(&json!("bad domain"))).is_err());
        let base = vec![Auth {
            key: K1.into(),
            domain: Some("https://env.kr".into()),
        }];
        assert_eq!(
            with_domain(base.clone(), None)[0].domain.as_deref(),
            Some("https://env.kr")
        );
        assert_eq!(
            with_domain(base, Some("https://req.kr"))[0]
                .domain
                .as_deref(),
            Some("https://req.kr")
        );
    }

    #[test]
    fn parse_key_list_splits_and_dedups() {
        let a = parse_key_list(" A1 , ,B2,A1 ", Some("https://x.kr"));
        assert_eq!(
            a.iter().map(|x| x.key.as_str()).collect::<Vec<_>>(),
            vec!["A1", "B2"]
        );
        assert_eq!(a[0].domain.as_deref(), Some("https://x.kr"));
    }

    #[tokio::test]
    async fn with_key_uses_only_given_keys_and_rotates_on_429() {
        let pool = vec![auth("RLKEYAAAAAAAAA01"), auth("RLKEYBBBBBBBBB02")];
        let seen = std::sync::Mutex::new(Vec::new());
        let r: Result<String> = with_key(&pool, |a| {
            seen.lock().unwrap().push(a.key.clone());
            async move {
                if a.key.ends_with("01") {
                    Err(RateLimited {
                        retry_after: Some(Duration::from_secs(1)),
                    }
                    .into())
                } else {
                    Ok(a.key)
                }
            }
        })
        .await;
        assert_eq!(r.unwrap(), "RLKEYBBBBBBBBB02");
        assert!(seen.lock().unwrap().iter().all(|k| k.starts_with("RLKEY")));
    }

    #[test]
    fn cap_output_truncates_with_notice() {
        let big = "가".repeat(MAX_OUTPUT); // 3바이트 문자 — 경계 처리 확인.
        let out = cap_output(big);
        assert!(out.contains("잘림"));
        assert!(out.len() < MAX_OUTPUT + 300);
        assert_eq!(cap_output("작음".into()), "작음");
    }

    #[test]
    fn validation_rules() {
        assert!(check_crs("crs", "EPSG:4326").is_ok());
        assert!(check_crs("crs", "4326").is_err());
        assert!(check_bbox("bbox", "126.9,37.5,127.0,37.6").is_ok());
        assert!(check_bbox("bbox", "126.9,37.5,127.0,37.6,EPSG:4326").is_ok());
        assert!(check_bbox("bbox", "1,2,3").is_err());
        assert!(check_pnu("1111018300101970001").is_ok());
        assert!(check_pnu("1234").is_err());
        assert!(check_pnu("11110183a").is_err());
        assert!(check_geom_filter("POINT(126.978 37.566)").is_ok());
        assert!(check_geom_filter("BOX(126.97,37.56,126.98,37.57)").is_ok());
        assert!(check_geom_filter("BOX(126.97,37.56,126.98,37.57,EPSG:4326)").is_err());
        assert!(check_geom_filter("DROP TABLE").is_err());
        assert!(check_geom_filter("POINT(1 2);x").is_err());
    }

    #[test]
    fn search_category_required() {
        let mut a = SearchInput {
            query: "종로구".into(),
            kind: "DISTRICT".into(),
            crs: "EPSG:4326".into(),
            ..Default::default()
        };
        assert!(validate_search(&a).unwrap_err().contains("category"));
        a.category = Some("L2".into());
        assert!(validate_search(&a).is_ok());
        a.kind = "ADDRESS".into();
        assert!(validate_search(&a).is_err());
        a.category = Some("PARCEL".into());
        assert!(validate_search(&a).is_ok());
        a.kind = "WHATEVER".into();
        assert!(validate_search(&a).is_err());
        let empty = SearchInput {
            query: "  ".into(),
            kind: "PLACE".into(),
            crs: "EPSG:4326".into(),
            ..Default::default()
        };
        assert!(validate_search(&empty).is_err());
    }

    #[test]
    fn data_validation() {
        let ok = DataInput {
            data_id: "LP_PA_CBND_BUBUN".into(),
            geom_filter: Some("POINT(126.978 37.566)".into()),
            crs: "EPSG:4326".into(),
            ..Default::default()
        };
        assert!(validate_data(&ok).is_ok());
        let mut no_filter = ok.clone();
        no_filter.geom_filter = None;
        assert!(validate_data(&no_filter).is_err());
        let mut bad_id = ok.clone();
        bad_id.data_id = "NOPE_LAYER".into();
        assert!(validate_data(&bad_id).is_err());
        let mut big = ok.clone();
        big.size = Some(5000);
        assert!(validate_data(&big).is_err());
        let mut bad_attr = ok;
        bad_attr.attr_filter = Some("pnu=1".into());
        assert!(validate_data(&bad_attr).is_err());
    }

    #[test]
    fn ned_validation() {
        let mut a = NedInput {
            operation: "getLandUseAttr".into(),
            ..Default::default()
        };
        assert!(validate_ned(&a).unwrap_err().contains("pnu"), "pnu 필수");
        a.pnu = Some("1111018300101970001".into());
        assert!(validate_ned(&a).is_ok());
        // WMS 계열 거부.
        let wms = NedInput {
            operation: "getLandUseWMS".into(),
            ..Default::default()
        };
        assert!(validate_ned(&wms).unwrap_err().contains("WMS"));
        // 인증 파라미터 주입 거부.
        let mut inj = a.clone();
        inj.params.insert("key".into(), json!("x"));
        assert!(validate_ned(&inj).is_err());
        let mut bad = a.clone();
        bad.params.insert("1bad".into(), json!("x"));
        assert!(validate_ned(&bad).is_err());
        let mut good = a;
        good.params.insert("numOfRows".into(), json!(100));
        assert_eq!(
            validate_ned(&good).unwrap().1,
            vec![("numOfRows".to_string(), "100".to_string())]
        );
        assert!(validate_ned(&NedInput {
            operation: "nope".into(),
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn offline_registries() {
        assert!(
            data_layers(Some("지적"), None, None).unwrap()["count"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(data_layers(None, None, Some("hex")).is_err());
        assert_eq!(
            data_describe("lp_pa_cbnd_bubun").unwrap()["data_id"],
            "LP_PA_CBND_BUBUN"
        );
        assert_eq!(ned_list(None, None, None).unwrap()["count"], 115);
        let one = ned_list(None, None, Some("getLandUseAttr")).unwrap();
        assert!(one["params"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "pnu"));
        assert!(!one["params"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "key"));
    }

    #[tokio::test]
    async fn zoning_reports_start_progress_before_work() {
        let seen = std::sync::Mutex::new(Vec::new());
        let rec = |p: u32, t: u32, m: &str| seen.lock().unwrap().push((p, t, m.to_string()));
        let client = Client::new().unwrap();
        // 범위 밖 좌표 — 네트워크 없이 검증 오류. 시작 알림은 그 전에 나간다.
        let r = zoning(&client, &[auth(K1)], None, Some(500.0), Some(500.0), &rec).await;
        assert!(r.is_err());
        let seen = seen.lock().unwrap();
        assert_eq!(seen.first().map(|s| (s.0, s.1)), Some((0, 2)));
    }

    #[test]
    fn list_extraction() {
        let d = json!({"data_id": "X", "pages_fetched": 1, "result": {"response": {"record": {"total": "2"},
            "result": {"featureCollection": {"features": [{"type": "Feature"}, {"type": "Feature"}]}}}}});
        assert_eq!(data_items(&d).unwrap().0.len(), 2);
        assert_eq!(data_info(&d)["server_total"], "2");
        let n = json!({"operation": "getIndvdLandPriceAttr", "kind": "data", "result": {"indvdLandPrices":
            {"field": [{"a": 1}, {"a": 2}], "totalCount": "24"}}});
        let (rows, geo) = ned_items(&n).unwrap();
        assert_eq!((rows.len(), geo), (2, false));
        assert_eq!(ned_info(&n)["server_total"], "24");
        let w = json!({"kind": "wfs", "result": {"features": [{"type": "Feature"}], "totalFeatures": 1}});
        assert!(ned_items(&w).unwrap().1);
    }

    #[test]
    fn land_use_summary_walks_tree() {
        let v = json!({"landUses": {"field": [
            {"prposAreaDstrcCodeNm": "제2종일반주거지역", "cnflcAtNm": "포함"},
            {"prposAreaDstrcCodeNm": "가축사육제한구역", "cnflcAtNm": "저촉"}
        ]}});
        let mut out = Vec::new();
        collect_land_use(&v, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["name"], "제2종일반주거지역");
    }
}
