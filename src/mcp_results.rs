//! `vworld-mcp` 큰 결과 다루기(규약 3단계) — 압축 형식 · 쪽 나누기 · 거르기 · 결과 파일 저장소.
//!
//! - 형식: `json`(속성 객체 배열) · `columns`(columns+rows+dict) · `csv`(머리줄+행) · `geojson`(FeatureCollection).
//!   `json`·`columns`·`csv` 는 도형 없이 속성만, `geojson` 은 도형 포함(좌표 자릿수 줄이기 가능).
//! - 응답이 상한(약 200KB)을 넘으면 첫 쪽 + `next_cursor`, 결과는 디스크에 저장(`<id>.jsonl`·`.csv`·`.geojson`·`.meta.json`).
//! - 저장소: `MCP_RESULTS_DIR`(기본 `~/.vworld/cache/mcp-results`), TTL `MCP_RESULTS_TTL`(기본 1시간),
//!   상한 `MCP_RESULTS_MAX_BYTES`(기본 1GB). 쓰기는 임시 파일 → rename(여러 프로세스 공유 가능).
//! - 파일에는 공개 응답 데이터만 — API 키·토큰은 넣지 않는다(키는 응답 본문에 없고, 인자 요약도 키 없음).

use anyhow::{anyhow, Result};
use base64::Engine;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 응답 데이터 칸 예산(바이트). 요약 칸·스크럽 여유를 빼고 상한 아래로.
pub const PAGE_BUDGET: usize = crate::mcp::MAX_OUTPUT - 4 * 1024;
/// `resources/read` 로 돌려줄 파일 상한.
pub const MAX_RESOURCE_BYTES: u64 = 20 * 1024 * 1024;
/// URI 앞부분.
pub const URI_PREFIX: &str = "vworld://results/";
/// 만료 안내.
pub const EXPIRED_MSG: &str = "결과가 만료됐어요 — 다시 조회하세요";

// ───────────────────────── 형식 ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Columns,
    Csv,
    Geojson,
}

impl Format {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Ok(Format::Json),
            "columns" => Ok(Format::Columns),
            "csv" => Ok(Format::Csv),
            "geojson" => Ok(Format::Geojson),
            _ => Err(anyhow!(
                "format 은 json · columns · csv · geojson 중 하나예요"
            )),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Columns => "columns",
            Format::Csv => "csv",
            Format::Geojson => "geojson",
        }
    }
}

/// 결과 모양 요청(도구 인자).
#[derive(Debug, Clone, Default)]
pub struct Shape {
    pub format: Option<Format>,
    /// 남길 속성 이름.
    pub properties: Option<Vec<String>>,
    /// 받아온 뒤 거르기 `{"속성": 값 | [값,..] | {"from":..,"to":..}}`.
    pub filter: Option<Map<String, Value>>,
    /// geojson 좌표 소수 자릿수(0~10).
    pub precision: Option<u32>,
    /// 작아도 파일로 저장.
    pub save: bool,
}

impl Shape {
    fn is_default(&self) -> bool {
        self.format.is_none()
            && self.properties.is_none()
            && self.filter.is_none()
            && self.precision.is_none()
            && !self.save
    }

    /// 인자 검증.
    pub fn validate(&self) -> Result<()> {
        if let Some(p) = &self.properties {
            if p.is_empty() || p.len() > 100 {
                return Err(anyhow!("properties 는 1~100개예요"));
            }
            if p.iter()
                .any(|n| n.is_empty() || n.len() > 64 || n.chars().any(char::is_control))
            {
                return Err(anyhow!("properties 이름은 1~64자예요"));
            }
        }
        if let Some(f) = &self.filter {
            if f.is_empty() || f.len() > 20 {
                return Err(anyhow!("filter 는 1~20개 조건이에요"));
            }
            for (k, v) in f {
                let ok = match v {
                    Value::String(_) | Value::Number(_) | Value::Bool(_) => true,
                    Value::Array(a) => {
                        !a.is_empty()
                            && a.len() <= 500
                            && a.iter().all(|x| !x.is_object() && !x.is_array())
                    }
                    Value::Object(o) => {
                        !o.is_empty()
                            && o.keys().all(|k| k == "from" || k == "to")
                            && o.values().all(|x| x.is_string() || x.is_number())
                    }
                    Value::Null => false,
                };
                if !ok {
                    return Err(anyhow!(
                        "filter.{k} 는 값 · 값 배열 · {{\"from\":..,\"to\":..}} 중 하나예요"
                    ));
                }
            }
        }
        if let Some(p) = self.precision {
            if p > 10 {
                return Err(anyhow!("precision 은 0~10 이에요"));
            }
        }
        Ok(())
    }
}

/// 항목(지형 Feature 또는 표 행)의 속성 객체.
fn props_of(item: &Value, geo: bool) -> Option<&Map<String, Value>> {
    if geo {
        item.get("properties").and_then(Value::as_object)
    } else {
        item.as_object()
    }
}

fn scalar_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn matches_cond(v: Option<&Value>, cond: &Value) -> bool {
    let Some(v) = v else { return false };
    let s = scalar_str(v);
    match cond {
        Value::Array(a) => a.iter().any(|c| scalar_str(c) == s),
        Value::Object(o) => {
            let cmp = |bound: &Value| -> Option<std::cmp::Ordering> {
                let b = scalar_str(bound);
                match (s.parse::<f64>(), b.parse::<f64>()) {
                    (Ok(x), Ok(y)) => x.partial_cmp(&y),
                    _ => Some(s.as_str().cmp(b.as_str())),
                }
            };
            o.get("from")
                .is_none_or(|f| cmp(f).is_some_and(|c| c.is_ge()))
                && o.get("to")
                    .is_none_or(|t| cmp(t).is_some_and(|c| c.is_le()))
        }
        other => scalar_str(other) == s,
    }
}

/// 거르기 · 속성 고르기를 적용한다.
pub fn apply_filter_select(items: Vec<Value>, geo: bool, shape: &Shape) -> Vec<Value> {
    items
        .into_iter()
        .filter(|it| {
            shape.filter.as_ref().is_none_or(|f| {
                let p = props_of(it, geo);
                f.iter()
                    .all(|(k, c)| matches_cond(p.and_then(|p| p.get(k)), c))
            })
        })
        .map(|mut it| {
            if let Some(keep) = &shape.properties {
                let target = if geo {
                    it.get_mut("properties")
                } else {
                    Some(&mut it)
                };
                if let Some(Value::Object(m)) = target {
                    m.retain(|k, _| keep.iter().any(|x| x == k));
                }
            }
            it
        })
        .collect()
}

fn round_coords(v: &mut Value, p: u32) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(|x| round_coords(x, p)),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                let m = 10f64.powi(p as i32);
                if let Some(r) = serde_json::Number::from_f64((f * m).round() / m) {
                    *n = r;
                }
            }
        }
        _ => {}
    }
}

fn feature_with_precision(item: &Value, precision: Option<u32>) -> Value {
    let mut f = item.clone();
    if let (Some(p), Some(c)) = (precision, f.pointer_mut("/geometry/coordinates")) {
        round_coords(c, p);
    }
    f
}

/// 속성 이름 모음(처음 나온 순서).
pub fn columns_of(items: &[Value], geo: bool) -> Vec<String> {
    let mut cols: Vec<String> = Vec::new();
    for it in items {
        if let Some(p) = props_of(it, geo) {
            for k in p.keys() {
                if !cols.iter().any(|c| c == k) {
                    cols.push(k.clone());
                }
            }
        }
    }
    cols
}

fn csv_cell(v: Option<&Value>) -> String {
    let s = v.map(scalar_str).unwrap_or_default();
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

fn csv_line(cols: &[String], p: Option<&Map<String, Value>>) -> String {
    cols.iter()
        .map(|c| csv_cell(p.and_then(|p| p.get(c))))
        .collect::<Vec<_>>()
        .join(",")
}

/// CSV 텍스트(머리줄 + 행).
pub fn to_csv(items: &[Value], geo: bool) -> String {
    let cols = columns_of(items, geo);
    let mut out = cols
        .iter()
        .map(|c| csv_cell(Some(&Value::String(c.clone()))))
        .collect::<Vec<_>>()
        .join(",");
    out.push('\n');
    for it in items {
        out.push_str(&csv_line(&cols, props_of(it, geo)));
        out.push('\n');
    }
    out
}

/// columns 형식: 반복되는 긴 문자열은 `dict` 로 한 번만(코드는 열마다 "0","1",…).
pub fn to_columns(items: &[Value], geo: bool) -> Value {
    let cols = columns_of(items, geo);
    let n = items.len();
    let mut dicts: Map<String, Value> = Map::new();
    let mut encoders: Vec<Option<HashMap<String, String>>> = Vec::with_capacity(cols.len());
    for c in &cols {
        let vals: Vec<&str> = items
            .iter()
            .filter_map(|it| props_of(it, geo).and_then(|p| p.get(c)))
            .filter_map(Value::as_str)
            .collect();
        let all_str = items
            .iter()
            .filter_map(|it| props_of(it, geo).and_then(|p| p.get(c)))
            .all(|v| v.is_string() || v.is_null());
        let mut distinct: Vec<&str> = Vec::new();
        let mut seen: HashMap<&str, ()> = HashMap::new();
        for v in &vals {
            if seen.insert(v, ()).is_none() {
                distinct.push(v);
            }
        }
        let avg = if vals.is_empty() {
            0
        } else {
            vals.iter().map(|v| v.len()).sum::<usize>() / vals.len()
        };
        if all_str && n >= 2 && !vals.is_empty() && distinct.len() * 2 <= vals.len() && avg >= 6 {
            let mut enc = HashMap::new();
            let mut d = Map::new();
            for (i, v) in distinct.iter().enumerate() {
                enc.insert(v.to_string(), i.to_string());
                d.insert(i.to_string(), Value::String(v.to_string()));
            }
            dicts.insert(c.clone(), Value::Object(d));
            encoders.push(Some(enc));
        } else {
            encoders.push(None);
        }
    }
    let rows: Vec<Value> = items
        .iter()
        .map(|it| {
            let p = props_of(it, geo);
            Value::Array(
                cols.iter()
                    .zip(&encoders)
                    .map(|(c, enc)| {
                        let v = p.and_then(|p| p.get(c)).cloned().unwrap_or(Value::Null);
                        match (enc, &v) {
                            (Some(e), Value::String(s)) => Value::String(e[s].clone()),
                            _ => v,
                        }
                    })
                    .collect(),
            )
        })
        .collect();
    json!({"columns": cols, "rows": rows, "dict": dicts})
}

/// columns 형식을 객체 배열로 되살린다(시험·클라이언트 참고용).
pub fn columns_to_objects(v: &Value) -> Vec<Map<String, Value>> {
    let cols: Vec<&str> = v["columns"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    v["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| {
                    let mut m = Map::new();
                    for (i, c) in cols.iter().enumerate() {
                        let mut x = r[i].clone();
                        if let (Some(code), Some(d)) = (x.as_str(), v["dict"].get(*c)) {
                            if let Some(name) = d.get(code) {
                                x = name.clone();
                            }
                        }
                        m.insert(c.to_string(), x);
                    }
                    m
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 한 쪽을 JSON 조각(문자열)으로 그린다.
fn render(items: &[Value], geo: bool, format: Format, precision: Option<u32>) -> String {
    match format {
        Format::Json => {
            let objs: Vec<Value> = items
                .iter()
                .map(|it| {
                    props_of(it, geo)
                        .map(|p| Value::Object(p.clone()))
                        .unwrap_or(Value::Null)
                })
                .collect();
            serde_json::to_string(&objs).unwrap_or_default()
        }
        Format::Columns => to_columns(items, geo).to_string(),
        Format::Csv => serde_json::to_string(&to_csv(items, geo)).unwrap_or_default(),
        Format::Geojson => {
            let feats: Vec<Value> = items
                .iter()
                .map(|f| feature_with_precision(f, precision))
                .collect();
            json!({"type": "FeatureCollection", "features": feats}).to_string()
        }
    }
}

/// 예산 안에 들어가는 만큼(최대 `max_rows`) 그린다. 한 행이 예산보다 커도 최소 1행.
pub fn render_fit(
    items: &[Value],
    geo: bool,
    format: Format,
    precision: Option<u32>,
    max_rows: usize,
    budget: usize,
) -> (String, usize) {
    if items.is_empty() {
        return (render(items, geo, format, precision), 0);
    }
    let cap = items.len().min(max_rows.max(1));
    let full = render(&items[..cap], geo, format, precision);
    if full.len() <= budget {
        return (full, cap);
    }
    // 대략 크기 비율로 시작해 줄여 간다.
    let mut n = ((cap as f64) * (budget as f64 / full.len() as f64) * 0.95)
        .floor()
        .max(1.0) as usize;
    loop {
        let s = render(&items[..n], geo, format, precision);
        if s.len() <= budget || n == 1 {
            return (s, n);
        }
        n = (n * 9 / 10).max(1).min(n - 1);
    }
}

// ───────────────────────── 커서 ─────────────────────────

/// 불투명 커서: base64url(JSON {i: id, o: offset, f: format, p: precision}).
pub fn encode_cursor(id: &str, offset: usize, format: Format, precision: Option<u32>) -> String {
    let v = json!({"i": id, "o": offset, "f": format.as_str(), "p": precision});
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string())
}

pub fn decode_cursor(c: &str) -> Result<(String, usize, Format, Option<u32>)> {
    let bad = || anyhow!("cursor 가 올바르지 않아요");
    if c.is_empty() || c.len() > 512 {
        return Err(bad());
    }
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(c)
        .map_err(|_| bad())?;
    let v: Value = serde_json::from_slice(&raw).map_err(|_| bad())?;
    let id = v["i"]
        .as_str()
        .filter(|i| valid_id(i))
        .ok_or_else(bad)?
        .to_string();
    let off = v["o"].as_u64().ok_or_else(bad)? as usize;
    let f = Format::parse(v["f"].as_str().ok_or_else(bad)?).map_err(|_| bad())?;
    let p = v["p"].as_u64().map(|p| p.min(10) as u32);
    Ok((id, off, f, p))
}

// ───────────────────────── 저장소 ─────────────────────────

/// 결과 저장소 설정.
#[derive(Debug, Clone)]
pub struct Store {
    pub dir: PathBuf,
    pub ttl: Duration,
    pub max_bytes: u64,
}

fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (num, mul) = match s.chars().last()? {
        's' => (&s[..s.len() - 1], 1),
        'm' => (&s[..s.len() - 1], 60),
        'h' => (&s[..s.len() - 1], 3600),
        _ => (s, 1),
    };
    num.trim()
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .map(|n| Duration::from_secs(n * mul))
}

impl Store {
    pub fn new(dir: PathBuf, ttl: Duration, max_bytes: u64) -> Self {
        Store {
            dir,
            ttl,
            max_bytes,
        }
    }

    /// `MCP_RESULTS_DIR` · `MCP_RESULTS_TTL`(초, 또는 30m·1h) · `MCP_RESULTS_MAX_BYTES`.
    pub fn from_env() -> Self {
        let dir = std::env::var_os("MCP_RESULTS_DIR")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                crate::config::default_config_path()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("cache").join("mcp-results")))
            })
            .unwrap_or_else(|| std::env::temp_dir().join("vworld-mcp-results"));
        let ttl = std::env::var("MCP_RESULTS_TTL")
            .ok()
            .and_then(|s| parse_duration(&s))
            .unwrap_or(Duration::from_secs(3600));
        let max_bytes = std::env::var("MCP_RESULTS_MAX_BYTES")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(1024 * 1024 * 1024);
        Store {
            dir,
            ttl,
            max_bytes,
        }
    }

    fn path(&self, id: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{id}.{ext}"))
    }

    /// 내려받기 파일 경로(이름 검증). ext 는 csv · geojson 만.
    pub fn download_path(&self, file: &str) -> Option<(PathBuf, &'static str)> {
        let (id, ext) = file.rsplit_once('.')?;
        let mime = match ext {
            "csv" => "text/csv",
            "geojson" => "application/geo+json",
            _ => return None,
        };
        if !valid_id(id) {
            return None;
        }
        let p = self.path(id, ext);
        (p.is_file() && self.load_meta(id).is_ok()).then_some((p, mime))
    }

    /// 원자적 쓰기(임시 파일 → rename).
    fn write_atomic(&self, id: &str, ext: &str, data: &[u8]) -> Result<u64> {
        let tmp = self.dir.join(format!(".{id}.{ext}.tmp-{}", new_id()));
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(data)?;
            f.sync_all().ok();
        }
        std::fs::rename(&tmp, self.path(id, ext))?;
        Ok(data.len() as u64)
    }

    /// 항목들을 저장하고 메타를 돌려준다. 메타는 맨 마지막에 쓴다(메타가 보이면 파일이 다 있음).
    pub fn save(
        &self,
        tool: &str,
        args_summary: &Value,
        items: &[Value],
        geo: bool,
    ) -> Result<Value> {
        std::fs::create_dir_all(&self.dir)?;
        let id = new_id();
        let mut jsonl = Vec::new();
        for it in items {
            serde_json::to_writer(&mut jsonl, it)?;
            jsonl.push(b'\n');
        }
        let mut bytes = self.write_atomic(&id, "jsonl", &jsonl)?;
        let csv = to_csv(items, geo);
        let csv_bytes = self.write_atomic(&id, "csv", csv.as_bytes())?;
        bytes += csv_bytes;
        let mut formats = vec!["jsonl", "csv"];
        let mut geo_bytes = None;
        if geo {
            let fc = json!({"type": "FeatureCollection", "features": items}).to_string();
            let b = self.write_atomic(&id, "geojson", fc.as_bytes())?;
            bytes += b;
            geo_bytes = Some(b);
            formats.push("geojson");
        }
        let meta = json!({
            "id": id, "tool": tool, "created": now_secs(), "rows": items.len(),
            "columns": columns_of(items, geo), "geo": geo, "bytes": bytes,
            "csv_bytes": csv_bytes, "geojson_bytes": geo_bytes,
            "formats": formats, "args_summary": args_summary,
        });
        self.write_atomic(&id, "meta.json", meta.to_string().as_bytes())?;
        Ok(meta)
    }

    /// 메타 읽기. 없거나 TTL 지났으면 만료 오류.
    pub fn load_meta(&self, id: &str) -> Result<Value> {
        if !valid_id(id) {
            return Err(anyhow!(EXPIRED_MSG));
        }
        let text = std::fs::read_to_string(self.path(id, "meta.json"))
            .map_err(|_| anyhow!(EXPIRED_MSG))?;
        let meta: Value = serde_json::from_str(&text).map_err(|_| anyhow!(EXPIRED_MSG))?;
        let created = meta["created"].as_u64().unwrap_or(0);
        if now_secs().saturating_sub(created) > self.ttl.as_secs() {
            return Err(anyhow!(EXPIRED_MSG));
        }
        Ok(meta)
    }

    /// 저장된 행을 offset 부터 max 개 읽는다.
    pub fn read_rows(&self, id: &str, offset: usize, max: usize) -> Result<Vec<Value>> {
        let f = std::fs::File::open(self.path(id, "jsonl")).map_err(|_| anyhow!(EXPIRED_MSG))?;
        let mut out = Vec::new();
        for line in std::io::BufReader::new(f).lines().skip(offset).take(max) {
            out.push(serde_json::from_str(&line?)?);
        }
        Ok(out)
    }

    /// 정리: TTL 지난 결과·오래된 임시 파일을 지우고, 전체가 상한을 넘으면 오래된 것부터 지운다.
    pub fn cleanup(&self) -> Result<(usize, u64)> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return Ok((0, 0));
        };
        let now = SystemTime::now();
        // id → (생성 시각, 파일들, 크기)
        let mut groups: HashMap<String, (SystemTime, Vec<PathBuf>, u64)> = HashMap::new();
        let mut removed = 0usize;
        for e in rd.flatten() {
            let p = e.path();
            let Some(name) = p.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
                continue;
            };
            let Ok(md) = e.metadata() else { continue };
            if !md.is_file() {
                continue;
            }
            let mtime = md.modified().unwrap_or(now);
            if name.starts_with('.') && name.contains(".tmp-") {
                if now.duration_since(mtime).unwrap_or_default() > self.ttl {
                    let _ = std::fs::remove_file(&p);
                }
                continue;
            }
            let Some(id) = name.split('.').next().filter(|i| valid_id(i)) else {
                continue;
            };
            let g = groups
                .entry(id.to_string())
                .or_insert((mtime, Vec::new(), 0));
            g.0 = g.0.min(mtime);
            g.1.push(p);
            g.2 += md.len();
        }
        let mut live: Vec<(SystemTime, u64, Vec<PathBuf>)> = Vec::new();
        for (_, (t, files, size)) in groups {
            if now.duration_since(t).unwrap_or_default() > self.ttl {
                files.iter().for_each(|f| drop(std::fs::remove_file(f)));
                removed += 1;
            } else {
                live.push((t, size, files));
            }
        }
        live.sort_by_key(|g| g.0);
        let mut total: u64 = live.iter().map(|g| g.1).sum();
        for (_, size, files) in &live {
            if total <= self.max_bytes {
                break;
            }
            files.iter().for_each(|f| drop(std::fs::remove_file(f)));
            total -= size;
            removed += 1;
        }
        Ok((removed, total))
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 무작위 256bit id(base64url, 43자).
pub fn new_id() -> String {
    let mut b = Vec::with_capacity(32);
    b.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    b.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

pub fn valid_id(id: &str) -> bool {
    (22..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ───────────────────────── 응답 모양 ─────────────────────────

/// 도구 결과(텍스트 + resource_link 항목들).
#[derive(Debug, Clone)]
pub struct Shaped {
    pub text: String,
    /// MCP `resource_link` 내용 항목(JSON).
    pub links: Vec<Value>,
}

/// 저장된 결과의 내려받기 안내와 resource_link 항목.
fn downloads(meta: &Value) -> (Value, Vec<Value>) {
    let id = meta["id"].as_str().unwrap_or_default();
    let rows = meta["rows"].as_u64().unwrap_or(0);
    let mut dl = Map::new();
    let mut links = Vec::new();
    let mut add = |ext: &str, mime: &str, size: &Value| {
        let uri = format!("{URI_PREFIX}{id}.{ext}");
        dl.insert(
            ext.into(),
            json!({"uri": uri, "http_path": format!("/results/{id}.{ext}"), "bytes": size}),
        );
        links.push(
            json!({"type": "resource_link", "uri": uri, "name": format!("{id}.{ext}"),
            "mimeType": mime, "size": size, "description": format!("전체 {rows}행")}),
        );
    };
    add("csv", "text/csv", &meta["csv_bytes"]);
    if meta["geo"].as_bool() == Some(true) {
        add("geojson", "application/geo+json", &meta["geojson_bytes"]);
    }
    (Value::Object(dl), links)
}

/// 요약 칸을 맨 앞에 두고 JSON 텍스트를 조립한다(serde_json 맵은 이름순이라 직접 잇는다).
#[allow(clippy::too_many_arguments)]
pub fn envelope(
    total: usize,
    returned: usize,
    format: Format,
    result_id: Option<&str>,
    next_cursor: Option<&str>,
    download: Option<&Value>,
    info: Option<&Value>,
    data: &str,
) -> String {
    let mut s = format!(
        "{{\"total_rows\":{total},\"returned_rows\":{returned},\"truncated\":{},\"format\":\"{}\"",
        returned < total,
        format.as_str()
    );
    if let Some(id) = result_id {
        s.push_str(&format!(",\"result_id\":{}", Value::String(id.into())));
    }
    if let Some(c) = next_cursor {
        s.push_str(&format!(",\"next_cursor\":{}", Value::String(c.into())));
    }
    if let Some(d) = download {
        s.push_str(&format!(",\"download\":{d}"));
    }
    if let Some(i) = info {
        s.push_str(&format!(",\"info\":{i}"));
    }
    s.push_str(&format!(",\"data\":{data}}}"));
    s
}

/// 도구 결과를 모양대로 만든다.
///
/// - 모양 인자가 없고 원래 결과가 상한 안이면 원래 결과 그대로(호환).
/// - 목록(`items`)이 있으면 형식대로 첫 쪽. 상한을 넘거나 `save` 면 저장 + `next_cursor` + 내려받기.
pub fn shape_result(
    store: &Store,
    tool: &str,
    args_summary: &Value,
    legacy: &Value,
    items: Option<(Vec<Value>, bool)>,
    info: &Value,
    shape: &Shape,
) -> Result<Shaped> {
    shape.validate()?;
    let legacy_text = || serde_json::to_string(legacy).unwrap_or_default();
    if shape.is_default() {
        let t = legacy_text();
        if t.len() <= crate::mcp::MAX_OUTPUT || items.is_none() {
            return Ok(Shaped {
                text: t,
                links: vec![],
            });
        }
    }
    let Some((items, geo)) = items else {
        // 목록이 아닌 결과 — 형식을 적용할 수 없으니 원래대로.
        return Ok(Shaped {
            text: legacy_text(),
            links: vec![],
        });
    };
    let format = shape.format.unwrap_or(Format::Columns);
    if format == Format::Geojson && !geo {
        return Err(anyhow!(
            "geojson 은 도형이 있는 결과(vworld_data, WFS 계열)에만 쓸 수 있어요"
        ));
    }
    let items = apply_filter_select(items, geo, shape);
    let total = items.len();
    let (data, n) = render_fit(
        &items,
        geo,
        format,
        shape.precision,
        usize::MAX,
        PAGE_BUDGET,
    );
    if n == total && !shape.save {
        return Ok(Shaped {
            text: envelope(total, n, format, None, None, None, Some(info), &data),
            links: vec![],
        });
    }
    let meta = store.save(tool, args_summary, &items, geo)?;
    let id = meta["id"].as_str().unwrap_or_default().to_string();
    let cursor = (n < total).then(|| encode_cursor(&id, n, format, shape.precision));
    let (dl, links) = downloads(&meta);
    Ok(Shaped {
        text: envelope(
            total,
            n,
            format,
            Some(&id),
            cursor.as_deref(),
            Some(&dl),
            Some(info),
            &data,
        ),
        links,
    })
}

/// 저장된 결과의 다음 쪽(다시 조회하지 않음, 키 불필요).
pub fn result_page(store: &Store, cursor: &str, limit: Option<usize>) -> Result<Shaped> {
    let (id, offset, format, precision) = decode_cursor(cursor)?;
    let meta = store.load_meta(&id)?;
    let total = meta["rows"].as_u64().unwrap_or(0) as usize;
    let geo = meta["geo"].as_bool() == Some(true);
    let limit = limit.unwrap_or(usize::MAX).clamp(1, 100_000);
    let rows = store.read_rows(&id, offset, limit)?;
    let (data, n) = render_fit(&rows, geo, format, precision, limit, PAGE_BUDGET);
    let next = offset + n;
    let cursor = (next < total).then(|| encode_cursor(&id, next, format, precision));
    let (dl, links) = downloads(&meta);
    let info = json!({"offset": offset, "tool": meta["tool"]});
    Ok(Shaped {
        text: envelope(
            total,
            n,
            format,
            Some(&id),
            cursor.as_deref(),
            Some(&dl),
            Some(&info),
            &data,
        ),
        links,
    })
}

/// `resources/read`: `vworld://results/<id>.<csv|geojson>` → (mime, 내용).
pub fn read_resource(store: &Store, uri: &str) -> Result<(&'static str, String)> {
    let file = uri
        .strip_prefix(URI_PREFIX)
        .ok_or_else(|| anyhow!("모르는 리소스예요: {URI_PREFIX}<id>.csv|geojson 만 있어요"))?;
    let (path, mime) = store
        .download_path(file)
        .ok_or_else(|| anyhow!(EXPIRED_MSG))?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size > MAX_RESOURCE_BYTES {
        return Err(anyhow!(
            "파일이 {size}바이트로 20MB 를 넘어요 — HTTP GET /results/{file} 로 내려받으세요"
        ));
    }
    Ok((mime, std::fs::read_to_string(&path)?))
}

/// 결과 파일 경로가 저장소 안에 있는지(HTTP 내려받기용).
pub fn is_inside(store: &Store, p: &Path) -> bool {
    p.parent() == Some(store.dir.as_path())
}

// ───────────────────────── 시험 ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("vworld-mcp-test-{name}-{}", new_id()));
        Store::new(dir, Duration::from_secs(3600), 1 << 30)
    }

    fn features(n: usize) -> Vec<Value> {
        (0..n)
            .map(|i| {
                json!({"type": "Feature",
                    "geometry": {"type": "Point", "coordinates": [126.123456789 + i as f64 * 1e-6, 37.5]},
                    "properties": {"pnu": format!("11140103001{:08}", i), "addr": format!("서울특별시 중구 태평로1가 {}", i % 3),
                        "jibun": format!("{i}대"), "area": i as f64 * 1.5}})
            })
            .collect()
    }

    fn page_rows(text: &str) -> (Value, usize) {
        let v: Value = serde_json::from_str(text).unwrap();
        let n = v["returned_rows"].as_u64().unwrap() as usize;
        (v, n)
    }

    #[test]
    fn small_result_keeps_legacy_shape() {
        let store = tmp_store("legacy");
        let legacy = json!({"data_id": "X", "result": {"a": 1}});
        let s = shape_result(
            &store,
            "t",
            &json!({}),
            &legacy,
            Some((features(3), true)),
            &json!({}),
            &Shape::default(),
        )
        .unwrap();
        assert_eq!(s.text, legacy.to_string());
        assert!(s.links.is_empty());
        assert!(!store.dir.exists(), "작은 결과는 저장하지 않음");
    }

    #[test]
    fn big_result_pages_collect_to_total_in_every_format() {
        let store = tmp_store("pages");
        let items = features(6000);
        let legacy = json!({"result": {"features": items.clone()}});
        for fmt in [
            None,
            Some(Format::Json),
            Some(Format::Csv),
            Some(Format::Geojson),
        ] {
            let shape = Shape {
                format: fmt,
                ..Default::default()
            };
            let s = shape_result(
                &store,
                "vworld_data",
                &json!({"data_id":"X"}),
                &legacy,
                Some((items.clone(), true)),
                &json!({}),
                &shape,
            )
            .unwrap();
            assert!(
                s.text.len() <= crate::mcp::MAX_OUTPUT,
                "{fmt:?} 첫 쪽이 상한을 넘음"
            );
            assert!(s.text.starts_with("{\"total_rows\":6000,"));
            let (v, mut got) = page_rows(&s.text);
            assert_eq!(v["truncated"], true);
            assert_eq!(v["format"], fmt.unwrap_or(Format::Columns).as_str());
            if fmt == Some(Format::Geojson) {
                assert_eq!(v["data"]["type"], "FeatureCollection");
            }
            assert!(s
                .links
                .iter()
                .any(|l| l["type"] == "resource_link" && l["mimeType"] == "text/csv"));
            let mut cursor = v["next_cursor"].as_str().map(str::to_string);
            let mut pages = 1;
            while let Some(c) = cursor {
                let p = result_page(&store, &c, None).unwrap();
                assert!(p.text.len() <= crate::mcp::MAX_OUTPUT);
                let (pv, n) = page_rows(&p.text);
                got += n;
                pages += 1;
                cursor = pv["next_cursor"].as_str().map(str::to_string);
            }
            assert_eq!(got, 6000, "{fmt:?} 모든 쪽 합계");
            assert!(pages > 1);
        }
        // limit 으로 작은 쪽.
        let s = shape_result(
            &store,
            "t",
            &json!({}),
            &legacy,
            Some((items.clone(), true)),
            &json!({}),
            &Shape {
                save: true,
                format: Some(Format::Json),
                ..Default::default()
            },
        )
        .unwrap();
        let (v, _) = page_rows(&s.text);
        let c = v["next_cursor"].as_str().unwrap();
        let p = result_page(&store, c, Some(7)).unwrap();
        assert_eq!(page_rows(&p.text).1, 7);
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[test]
    fn formats_round_trip_and_dict_restores_names() {
        let items = features(10);
        let cols = to_columns(&items, true);
        assert!(cols["dict"]["addr"].is_object(), "반복되는 긴 주소는 dict");
        let back = columns_to_objects(&cols);
        for (o, it) in back.iter().zip(&items) {
            assert_eq!(&Value::Object(o.clone()), &it["properties"]);
        }
        let csv = to_csv(&items, true);
        let mut lines = csv.lines();
        assert_eq!(lines.next().unwrap(), "addr,area,jibun,pnu");
        assert_eq!(
            lines.next().unwrap(),
            "서울특별시 중구 태평로1가 0,0.0,0대,1114010300100000000"
        );
        assert_eq!(csv.lines().count(), 11);
        let j: Vec<Value> =
            serde_json::from_str(&render(&items, true, Format::Json, None)).unwrap();
        assert_eq!(j[3], items[3]["properties"]);
        // CSV 따옴표.
        assert_eq!(csv_cell(Some(&json!("a,\"b\""))), "\"a,\"\"b\"\"\"");
        // 좌표 자릿수.
        let g: Value =
            serde_json::from_str(&render(&items[..1], true, Format::Geojson, Some(3))).unwrap();
        assert_eq!(
            g["features"][0]["geometry"]["coordinates"][0],
            json!(126.123)
        );
    }

    #[test]
    fn filter_and_properties_selection() {
        let items = features(20);
        let shape = Shape {
            filter: Some(
                serde_json::from_value(
                    json!({"addr": "서울특별시 중구 태평로1가 1", "area": {"from": 3, "to": 15}}),
                )
                .unwrap(),
            ),
            properties: Some(vec!["pnu".into(), "area".into()]),
            ..Default::default()
        };
        let out = apply_filter_select(items.clone(), true, &shape);
        // i%3==1 이고 1.5i ∈ [3,15] → i ∈ {4,7,10}
        let areas: Vec<f64> = out
            .iter()
            .map(|f| f["properties"]["area"].as_f64().unwrap())
            .collect();
        assert_eq!(areas, vec![6.0, 10.5, 15.0]);
        assert!(out
            .iter()
            .all(|f| f["properties"].as_object().unwrap().len() == 2));
        assert!(out[0]["geometry"].is_object(), "도형은 남음");
        let list = Shape {
            filter: Some(serde_json::from_value(json!({"jibun": ["1대", "2대"]})).unwrap()),
            ..Default::default()
        };
        assert_eq!(apply_filter_select(items, true, &list).len(), 2);
        assert!(Shape {
            filter: Some(serde_json::from_value(json!({"a": null})).unwrap()),
            ..Default::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn expired_and_cleanup() {
        let store = tmp_store("ttl");
        let meta = store.save("t", &json!({}), &features(5), true).unwrap();
        let id = meta["id"].as_str().unwrap().to_string();
        assert!(valid_id(&id) && id.len() >= 43);
        assert!(store.load_meta(&id).is_ok());
        let c = encode_cursor(&id, 2, Format::Csv, None);
        assert!(result_page(&store, &c, None).is_ok());
        // TTL 0 초과로 본다 → 만료.
        let short = Store::new(store.dir.clone(), Duration::from_secs(0), 1 << 30);
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(
            result_page(&short, &c, None).unwrap_err().to_string(),
            EXPIRED_MSG
        );
        let (removed, left) = short.cleanup().unwrap();
        assert_eq!((removed, left), (1, 0));
        assert!(std::fs::read_dir(&store.dir).unwrap().next().is_none());
        // 용량 상한: 오래된 것부터 지운다.
        let a = store.save("t", &json!({}), &features(50), true).unwrap();
        std::thread::sleep(Duration::from_millis(1100));
        let b = store.save("t", &json!({}), &features(50), true).unwrap();
        let tight = Store::new(
            store.dir.clone(),
            Duration::from_secs(3600),
            b["bytes"].as_u64().unwrap() + 4000,
        );
        let (removed, _) = tight.cleanup().unwrap();
        assert_eq!(removed, 1);
        assert!(store.load_meta(a["id"].as_str().unwrap()).is_err());
        assert!(store.load_meta(b["id"].as_str().unwrap()).is_ok());
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[test]
    fn resource_link_shape_and_read() {
        let store = tmp_store("res");
        let items = features(4);
        let s = shape_result(
            &store,
            "t",
            &json!({"data_id": "X"}),
            &json!({}),
            Some((items, true)),
            &json!({}),
            &Shape {
                save: true,
                ..Default::default()
            },
        )
        .unwrap();
        let v: Value = serde_json::from_str(&s.text).unwrap();
        let id = v["result_id"].as_str().unwrap();
        assert!(v.get("next_cursor").is_none(), "다 들어가면 커서 없음");
        let csv = s
            .links
            .iter()
            .find(|l| l["mimeType"] == "text/csv")
            .unwrap();
        assert_eq!(csv["type"], "resource_link");
        assert_eq!(csv["uri"], format!("vworld://results/{id}.csv"));
        assert_eq!(csv["name"], format!("{id}.csv"));
        assert_eq!(csv["description"], "전체 4행");
        assert!(csv["size"].as_u64().unwrap() > 0);
        let geo = s
            .links
            .iter()
            .find(|l| l["mimeType"] == "application/geo+json")
            .unwrap();
        let (mime, text) = read_resource(&store, geo["uri"].as_str().unwrap()).unwrap();
        assert_eq!(mime, "application/geo+json");
        let fc: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(fc["features"].as_array().unwrap().len(), 4);
        assert!(read_resource(&store, "vworld://results/../../etc/passwd.csv").is_err());
        assert!(read_resource(&store, &format!("vworld://results/{id}.jsonl")).is_err());
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[test]
    fn files_hold_no_keys() {
        // 저장되는 것은 응답 항목과 인자 요약뿐 — 키를 넘길 통로가 없다. 그래도 확인.
        let store = tmp_store("nokey");
        let key = "SECRETKEY-1234-5678-ABCD";
        let meta = store
            .save(
                "vworld_data",
                &json!({"data_id": "LP_PA_CBND_BUBUN"}),
                &features(3),
                true,
            )
            .unwrap();
        for e in std::fs::read_dir(&store.dir).unwrap().flatten() {
            let t = std::fs::read_to_string(e.path()).unwrap();
            assert!(!t.contains(key) && !t.contains("\"key\""));
        }
        assert!(meta["id"].as_str().unwrap().len() >= 43);
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[test]
    fn cursor_round_trip_and_garbage() {
        let id = new_id();
        let c = encode_cursor(&id, 42, Format::Geojson, Some(5));
        assert_eq!(
            decode_cursor(&c).unwrap(),
            (id, 42, Format::Geojson, Some(5))
        );
        assert!(decode_cursor("not-a-cursor").is_err());
        assert!(decode_cursor("").is_err());
        assert_eq!(parse_duration("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_duration("7200"), Some(Duration::from_secs(7200)));
    }
}
