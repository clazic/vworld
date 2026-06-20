//! NED 레지스트리 codegen — `references/data/ned_catalog.tsv`(단일 출처) → 정적 테이블 Rust 생성.
//!
//! 드리프트 가드(설계 §8-⑩): 행수 115 및 prefix 집계 36/33/46 불일치 시 빌드 실패.
//! 추가: `references/data/ned_params.tsv` → NedParam 정적 슬라이스 코드젠.
//!       1:1 정합성 가드: params.tsv의 모든 endpoint_op이 catalog.tsv 115개와 정확히 매핑.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

/// JSON 문자열에서 이스케이프 시퀀스를 처리해 실제 문자열 반환.
fn unescape_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('u') => {
                    // \uXXXX
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(n) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(n) {
                            out.push(ch);
                        }
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `"key": <value>` 패턴에서 문자열 값 추출.
/// value가 `"..."` 형태일 때만 사용.
fn extract_str_field<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\":", key);
    let pos = obj.find(&needle)?;
    let after = obj[pos + needle.len()..].trim_start();
    if after.starts_with('"') {
        // 닫는 따옴표 탐색 (이스케이프 고려)
        let inner = &after[1..];
        let mut end = 0;
        let mut escaped = false;
        for (i, ch) in inner.char_indices() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                end = i;
                break;
            }
        }
        Some(&inner[..end])
    } else {
        None
    }
}

/// `"required": true/false` 추출.
fn extract_bool_field(obj: &str, key: &str) -> bool {
    let needle = format!("\"{}\":", key);
    if let Some(pos) = obj.find(&needle) {
        let after = obj[pos + needle.len()..].trim_start();
        after.starts_with("true")
    } else {
        false
    }
}

/// JSON 배열 `[{...},{...},...]` 에서 객체 문자열 목록 파싱 (중첩 없음 가정).
fn parse_json_array_objects(json: &str) -> Vec<String> {
    let mut result = Vec::new();
    let trimmed = json.trim();
    // 배열 시작/끝 `[` `]` 제거
    if !trimmed.starts_with('[') {
        return result;
    }
    let inner = &trimmed[1..];
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    let mut in_str = false;
    let mut escaped = false;
    for (i, ch) in inner.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_str {
            if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        result.push(inner[s..=i].to_string());
                        start = None;
                    }
                }
            }
            _ => {}
        }
    }
    result
}

/// Rust 문자열 리터럴용 이스케이프 (큰따옴표·백슬래시만).
fn escape_rust_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn main() {
    println!("cargo:rerun-if-changed=references/data/ned_catalog.tsv");
    println!("cargo:rerun-if-changed=references/data/ned_params.tsv");

    // ── 1. ned_catalog.tsv 파싱 ──────────────────────────────────────────────
    let tsv = fs::read_to_string("references/data/ned_catalog.tsv")
        .expect("references/data/ned_catalog.tsv 읽기 실패");
    let mut rows: Vec<[String; 6]> = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    // catalog의 endpoint_op → 인덱스 (정합성 가드용)
    let mut catalog_ops: BTreeMap<String, usize> = BTreeMap::new();

    for (i, line) in tsv.lines().enumerate() {
        if i == 0 {
            continue; // 헤더.
        }
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        assert!(
            cols.len() >= 6,
            "ned_catalog.tsv 행 {} 컬럼 부족: {:?}",
            i + 1,
            cols
        );
        let kind = cols[1].to_string();
        *counts.entry(kind.clone()).or_default() += 1;
        let endpoint_op = cols[2].to_string();
        let idx = rows.len();
        catalog_ops.insert(endpoint_op.clone(), idx);
        rows.push([
            cols[0].to_string(),
            kind,
            endpoint_op,
            cols[3].to_string(),
            cols[4].to_string(),
            cols[5].to_string(),
        ]);
    }

    // 드리프트 가드.
    assert_eq!(rows.len(), 115, "NED 레지스트리 행수가 115가 아님: {}", rows.len());
    assert_eq!(counts.get("wms").copied().unwrap_or(0), 36, "wms prefix 집계 불일치");
    assert_eq!(counts.get("wfs").copied().unwrap_or(0), 33, "wfs prefix 집계 불일치");
    assert_eq!(counts.get("data").copied().unwrap_or(0), 46, "data prefix 집계 불일치");

    // ── 2. ned_params.tsv 파싱 ───────────────────────────────────────────────
    // 형식: endpoint_op\t<params_json> (헤더 없음, 115행)
    let params_tsv = fs::read_to_string("references/data/ned_params.tsv")
        .expect("references/data/ned_params.tsv 읽기 실패");

    // endpoint_op → Vec<(name, required, type, default, desc)>
    let mut params_map: BTreeMap<String, Vec<(String, bool, String, String, String)>> =
        BTreeMap::new();
    let mut params_ops_seen: BTreeMap<String, bool> = BTreeMap::new();

    for (i, line) in params_tsv.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let tab_pos = line.find('\t').unwrap_or_else(|| {
            panic!("ned_params.tsv 행 {} 탭 없음: {:?}", i + 1, line)
        });
        let op = line[..tab_pos].trim().to_string();
        let json_str = line[tab_pos + 1..].trim();

        assert!(
            !params_ops_seen.contains_key(&op),
            "ned_params.tsv 중복 endpoint_op: {}",
            op
        );
        params_ops_seen.insert(op.clone(), true);

        // parse_failed 처리
        if json_str.contains("\"parse_failed\"") {
            eprintln!("cargo:warning=ned_params.tsv: {} parse_failed — 빈 슬라이스 사용", op);
            params_map.insert(op, Vec::new());
            continue;
        }

        let objs = parse_json_array_objects(json_str);
        let mut params: Vec<(String, bool, String, String, String)> = Vec::new();
        for obj in &objs {
            let name = extract_str_field(obj, "name")
                .map(|s| unescape_json_str(s))
                .unwrap_or_default();
            let required = extract_bool_field(obj, "required");
            let typ = extract_str_field(obj, "type")
                .map(|s| unescape_json_str(s))
                .unwrap_or_default();
            let default = extract_str_field(obj, "default")
                .map(|s| unescape_json_str(s))
                .unwrap_or_default();
            let desc = extract_str_field(obj, "desc")
                .map(|s| unescape_json_str(s))
                .unwrap_or_default();
            params.push((name, required, typ, default, desc));
        }
        params_map.insert(op, params);
    }

    // 1:1 정합성 가드
    assert_eq!(
        params_map.len(),
        115,
        "ned_params.tsv 행수가 115가 아님: {}",
        params_map.len()
    );
    for op in catalog_ops.keys() {
        assert!(
            params_map.contains_key(op),
            "ned_params.tsv에 catalog op 누락: {}",
            op
        );
    }
    for op in params_map.keys() {
        assert!(
            catalog_ops.contains_key(op),
            "ned_params.tsv에 catalog에 없는 잉여 op: {}",
            op
        );
    }

    // ── 3. 코드젠 ────────────────────────────────────────────────────────────
    let mut out = String::new();
    out.push_str("// @generated by build.rs from references/data/ned_catalog.tsv + ned_params.tsv — 수정 금지.\n");

    // 각 op의 정적 NedParam 슬라이스 먼저 emit
    for r in &rows {
        let op = &r[2];
        let params = params_map.get(op).map(|v| v.as_slice()).unwrap_or(&[]);
        if params.is_empty() {
            continue;
        }
        let const_name = format!("NED_PARAMS_{}", op.to_uppercase());
        out.push_str(&format!(
            "static {}: &[NedParam] = &[\n",
            const_name
        ));
        for (name, required, typ, default, desc) in params {
            out.push_str(&format!(
                "    NedParam {{ name: \"{}\", required: {}, r#type: \"{}\", default: \"{}\", desc: \"{}\" }},\n",
                escape_rust_str(name),
                required,
                escape_rust_str(typ),
                escape_rust_str(default),
                escape_rust_str(desc),
            ));
        }
        out.push_str("];\n");
    }

    out.push_str("pub static NED_OPS: &[NedOp] = &[\n");
    for r in &rows {
        let op = &r[2];
        let params = params_map.get(op).map(|v| v.as_slice()).unwrap_or(&[]);
        let params_ref = if params.is_empty() {
            "&[]".to_string()
        } else {
            format!("NED_PARAMS_{}", op.to_uppercase())
        };
        out.push_str(&format!(
            "    NedOp {{ apinum: {}, kind: {:?}, endpoint_op: {:?}, cat1: {:?}, cat2: {:?}, name: {:?}, params: {} }},\n",
            r[0].parse::<u32>().unwrap_or(0),
            r[1].as_str(),
            r[2].as_str(),
            r[3].as_str(),
            r[4].as_str(),
            r[5].as_str(),
            params_ref,
        ));
    }
    out.push_str("];\n");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("ned_catalog_gen.rs");
    fs::write(dest, out).expect("생성 파일 쓰기 실패");

    // ── 2D 데이터 레이어 코드젠 ──────────────────────────────────────────────
    gen_twod_catalog(&out_dir);
}

/// 2D 데이터 158 레이어 코드젠.
/// `references/data/twod_catalog.tsv` + `references/data/twod_attrs.tsv` →
/// `OUT_DIR/twod_catalog_gen.rs`
fn gen_twod_catalog(out_dir: &str) {
    println!("cargo:rerun-if-changed=references/data/twod_catalog.tsv");
    println!("cargo:rerun-if-changed=references/data/twod_attrs.tsv");

    // ── 1. twod_catalog.tsv 파싱 ──────────────────────────────────────────
    // 형식: 헤더 + N행, 컬럼 data_id\tsvc_ide\tname\tcat\tgeom
    let cat_tsv = fs::read_to_string(Path::new("references/data/twod_catalog.tsv"))
        .expect("references/data/twod_catalog.tsv 읽기 실패");

    // (data_id, svc_ide, name, cat, geom)
    let mut catalog_rows: Vec<[String; 5]> = Vec::new();
    // data_id → index (정합성 가드용)
    let mut catalog_ids: BTreeMap<String, usize> = BTreeMap::new();

    for (i, line) in cat_tsv.lines().enumerate() {
        if i == 0 {
            continue; // 헤더
        }
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        assert!(
            cols.len() >= 5,
            "twod_catalog.tsv 행 {} 컬럼 부족: {:?}",
            i + 1,
            cols
        );
        let data_id = cols[0].to_string();
        let idx = catalog_rows.len();
        catalog_ids.insert(data_id.clone(), idx);
        catalog_rows.push([
            data_id,
            cols[1].to_string(),
            cols[2].to_string(),
            cols[3].to_string(),
            cols[4].to_string(),
        ]);
    }

    // ── 2. twod_attrs.tsv 파싱 ────────────────────────────────────────────
    // 형식: 헤더 없음, N행, 컬럼 data_id\t<json배열>
    // 각 배열 원소: {"name", "single_search"(bool), "type", "desc"}
    let attrs_tsv = fs::read_to_string(Path::new("references/data/twod_attrs.tsv"))
        .expect("references/data/twod_attrs.tsv 읽기 실패");

    // data_id → Vec<(name, single_search, type, desc)>
    let mut attrs_map: BTreeMap<String, Vec<(String, bool, String, String)>> = BTreeMap::new();
    let mut attrs_ids_seen: BTreeMap<String, bool> = BTreeMap::new();

    for (i, line) in attrs_tsv.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let tab_pos = line.find('\t').unwrap_or_else(|| {
            panic!("twod_attrs.tsv 행 {} 탭 없음: {:?}", i + 1, line)
        });
        let data_id = line[..tab_pos].trim().to_string();
        let json_str = line[tab_pos + 1..].trim();

        assert!(
            !attrs_ids_seen.contains_key(&data_id),
            "twod_attrs.tsv 중복 data_id: {}",
            data_id
        );
        attrs_ids_seen.insert(data_id.clone(), true);

        let objs = parse_json_array_objects(json_str);
        let mut attrs: Vec<(String, bool, String, String)> = Vec::new();
        for obj in &objs {
            let name = extract_str_field(obj, "name")
                .map(|s| unescape_json_str(s))
                .unwrap_or_default();
            let single_search = extract_bool_field(obj, "single_search");
            let typ = extract_str_field(obj, "type")
                .map(|s| unescape_json_str(s))
                .unwrap_or_default();
            let desc = extract_str_field(obj, "desc")
                .map(|s| unescape_json_str(s))
                .unwrap_or_default();
            attrs.push((name, single_search, typ, desc));
        }
        attrs_map.insert(data_id, attrs);
    }

    // ── 3. 카탈로그↔속성 양방향 1:1 정합성 가드 ──────────────────────────
    for id in catalog_ids.keys() {
        assert!(
            attrs_map.contains_key(id),
            "twod_attrs.tsv에 catalog data_id 누락: {}",
            id
        );
    }
    for id in attrs_map.keys() {
        assert!(
            catalog_ids.contains_key(id),
            "twod_attrs.tsv에 catalog에 없는 잉여 data_id: {}",
            id
        );
    }

    // ── 4. 코드젠 ─────────────────────────────────────────────────────────
    let mut out = String::new();
    out.push_str(
        "// @generated by build.rs from references/data/twod_catalog.tsv + twod_attrs.tsv — 수정 금지.\n",
    );

    // index 기반 const로 각 레이어의 속성 슬라이스 emit (C1: 데이터ID를 식별자로 쓰지 않음)
    for (idx, row) in catalog_rows.iter().enumerate() {
        let data_id = &row[0];
        let attrs = attrs_map.get(data_id).map(|v| v.as_slice()).unwrap_or(&[]);
        if attrs.is_empty() {
            continue;
        }
        let const_name = format!("TWOD_ATTRS_{}", idx);
        out.push_str(&format!("static {}: &[TwodAttr] = &[\n", const_name));
        for (name, single_search, typ, desc) in attrs {
            out.push_str(&format!(
                "    TwodAttr {{ name: \"{}\", single_search: {}, r#type: \"{}\", desc: \"{}\" }},\n",
                escape_rust_str(name),
                single_search,
                escape_rust_str(typ),
                escape_rust_str(desc),
            ));
        }
        out.push_str("];\n");
    }

    // 전체 레이어 정적 슬라이스 emit
    out.push_str("pub static TWOD_LAYERS: &[TwodLayer] = &[\n");
    for (idx, row) in catalog_rows.iter().enumerate() {
        let data_id = &row[0];
        let attrs = attrs_map.get(data_id).map(|v| v.as_slice()).unwrap_or(&[]);
        let attrs_ref = if attrs.is_empty() {
            "&[]".to_string()
        } else {
            format!("TWOD_ATTRS_{}", idx)
        };
        out.push_str(&format!(
            "    TwodLayer {{ data_id: {:?}, svc_ide: {:?}, name: {:?}, cat: {:?}, geom: {:?}, attrs: {} }},\n",
            row[0].as_str(),
            row[1].as_str(),
            row[2].as_str(),
            row[3].as_str(),
            row[4].as_str(),
            attrs_ref,
        ));
    }
    out.push_str("];\n");

    let dest = Path::new(out_dir).join("twod_catalog_gen.rs");
    fs::write(dest, out).expect("twod_catalog_gen.rs 쓰기 실패");
}
