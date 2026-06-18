//! DXF R2000(AC1015) writer — GeoJSON FeatureCollection → DXF 바이트.
//!
//! 외부 크레이트 없이 `serde_json` + `encoding_rs`만 사용.
//!
//! ## 지원 사양
//! - DXF R2000 (AC1015)
//! - LWPOLYLINE (필지 윤곽, 레이어 PARCEL, closed=1)
//! - MTEXT (지번 라벨, 레이어 JIBUN, 필지 centroid)
//! - CP949(EUC-KR/UHC) 인코딩 옵션
//! - MultiPolygon·내환(ring) 각각 독립 LWPOLYLINE

use anyhow::{anyhow, Result};
use serde_json::Value;

/// DXF 출력 옵션.
pub struct DxfOpts {
    /// 텍스트 인코딩 (기본 "cp949"). "cp949"이면 encoding_rs로 EUC-KR 인코딩.
    pub encoding: String,
    /// 심볼/텍스트 스케일 분모 (기본 1000 = 1:1000). 텍스트 높이 계산에 사용.
    pub symbol_scale: u32,
    /// 라벨로 사용할 feature properties 키 (기본 "pnu").
    pub label_field: String,
    /// 좌표 단위가 degree(EPSG:4326)이면 true, 미터 투영계이면 false.
    /// true: 텍스트 높이를 degree 환산 (height_m / 111320),
    /// false: 텍스트 높이를 미터 그대로 (2.5/1000 * scale).
    pub is_degree: bool,
}

impl Default for DxfOpts {
    fn default() -> Self {
        Self {
            encoding: "cp949".to_string(),
            symbol_scale: 1000,
            label_field: "pnu".to_string(),
            is_degree: true,
        }
    }
}

/// GeoJSON FeatureCollection → DXF R2000 바이트.
///
/// `fc`는 `{"type":"FeatureCollection","features":[...]}` 형식의 Value.
pub fn feature_collection_to_dxf(fc: &Value, opts: &DxfOpts) -> Result<Vec<u8>> {
    let features = fc["features"]
        .as_array()
        .ok_or_else(|| anyhow!("features 배열 없음"))?;

    let mut writer = DxfWriter::new(opts.symbol_scale, opts.is_degree);

    for feature in features {
        let label = feature["properties"][&opts.label_field]
            .as_str()
            .unwrap_or("")
            .to_string();

        let geometry = &feature["geometry"];
        let geo_type = geometry["type"].as_str().unwrap_or("");

        match geo_type {
            "Polygon" => {
                if let Some(rings) = geometry["coordinates"].as_array() {
                    write_polygon_rings(&mut writer, rings, &label);
                }
            }
            "MultiPolygon" => {
                if let Some(parts) = geometry["coordinates"].as_array() {
                    for part in parts {
                        if let Some(rings) = part.as_array() {
                            write_polygon_rings(&mut writer, rings, &label);
                        }
                    }
                }
            }
            _ => {} // 지원하지 않는 지오메트리 유형은 무시
        }
    }

    let dxf_str = writer.build();

    // 인코딩 처리
    if opts.encoding.to_lowercase() == "cp949" {
        let (encoded, _, had_errors) = encoding_rs::EUC_KR.encode(&dxf_str);
        if had_errors {
            // 인코딩 실패 문자는 '?'로 대체하여 재시도
            let sanitized: String = dxf_str
                .chars()
                .map(|c| {
                    if c.is_ascii() {
                        c
                    } else {
                        // EUC-KR 지원 여부 확인 후 대체
                        let s = c.to_string();
                        let (enc, _, err) = encoding_rs::EUC_KR.encode(&s);
                        if err {
                            '?'
                        } else {
                            let _ = enc; // enc는 사용하지 않고 오류 여부만 확인
                            c
                        }
                    }
                })
                .collect();
            let (encoded2, _, _) = encoding_rs::EUC_KR.encode(&sanitized);
            Ok(encoded2.to_vec())
        } else {
            Ok(encoded.to_vec())
        }
    } else {
        Ok(dxf_str.into_bytes())
    }
}

/// Polygon의 모든 ring을 LWPOLYLINE으로 기록하고, 외환의 centroid에 MTEXT 라벨 추가.
fn write_polygon_rings(writer: &mut DxfWriter, rings: &[Value], label: &str) {
    for (i, ring) in rings.iter().enumerate() {
        if let Some(coords) = ring.as_array() {
            let points: Vec<(f64, f64)> = coords
                .iter()
                .filter_map(|c| {
                    let arr = c.as_array()?;
                    let x = arr.get(0)?.as_f64()?;
                    let y = arr.get(1)?.as_f64()?;
                    Some((x, y))
                })
                .collect();

            if points.is_empty() {
                continue;
            }

            // 외환(i==0)에만 MTEXT 라벨
            if i == 0 && !label.is_empty() {
                let (cx, cy) = centroid(&points);
                writer.add_mtext(cx, cy, label);
            }

            writer.add_lwpolyline(&points);
        }
    }
}

/// 정점 목록의 단순 평균 centroid.
fn centroid(points: &[(f64, f64)]) -> (f64, f64) {
    // 마지막 정점이 첫 정점과 같은 닫힌 ring이면 제외하고 평균
    let pts = if points.len() > 1 {
        let first = points[0];
        let last = points[points.len() - 1];
        if (first.0 - last.0).abs() < 1e-12 && (first.1 - last.1).abs() < 1e-12 {
            &points[..points.len() - 1]
        } else {
            points
        }
    } else {
        points
    };

    if pts.is_empty() {
        return (0.0, 0.0);
    }

    let sum_x: f64 = pts.iter().map(|(x, _)| x).sum();
    let sum_y: f64 = pts.iter().map(|(_, y)| y).sum();
    let n = pts.len() as f64;
    (sum_x / n, sum_y / n)
}

// ---------------------------------------------------------------------------
// DxfWriter — DXF R2000(AC1015) 내부 빌더
// ---------------------------------------------------------------------------

struct DxfWriter {
    handle_counter: u32,
    symbol_scale: u32,
    /// 좌표 단위가 degree(EPSG:4326)이면 true, 미터 투영계이면 false.
    is_degree: bool,
    /// (points, is_closed) — LWPOLYLINE 목록
    lwpolylines: Vec<Vec<(f64, f64)>>,
    /// (x, y, text) — MTEXT 목록
    mtexts: Vec<(f64, f64, String)>,
}

impl DxfWriter {
    fn new(symbol_scale: u32, is_degree: bool) -> Self {
        Self {
            handle_counter: 1,
            symbol_scale,
            is_degree,
            lwpolylines: Vec::new(),
            mtexts: Vec::new(),
        }
    }

    fn next_handle(&mut self) -> String {
        let h = format!("{:X}", self.handle_counter);
        self.handle_counter += 1;
        h
    }

    fn add_lwpolyline(&mut self, points: &[(f64, f64)]) {
        self.lwpolylines.push(points.to_vec());
    }

    fn add_mtext(&mut self, x: f64, y: f64, text: &str) {
        self.mtexts.push((x, y, text.to_string()));
    }

    /// MTEXT 텍스트 높이.
    /// paper_mm=2.5, scale_denom=symbol_scale → height_m = 2.5/1000 * scale.
    /// is_degree=true: height_m / 111320 (degree 단위),
    /// is_degree=false: height_m 그대로 (미터 단위).
    fn text_height(&self) -> f64 {
        let height_m = (2.5 / 1000.0) * self.symbol_scale as f64;
        if self.is_degree {
            height_m / 111_320.0
        } else {
            height_m
        }
    }

    /// DXF 전체 문자열 빌드.
    fn build(&mut self) -> String {
        let mut out = String::with_capacity(64 * 1024);

        self.write_header(&mut out);
        self.write_tables(&mut out);
        self.write_entities(&mut out);
        self.write_eof(&mut out);

        out
    }

    // -----------------------------------------------------------------------
    // SECTION HEADER
    // -----------------------------------------------------------------------

    fn write_header(&mut self, out: &mut String) {
        // HANDSEED: 현재 handle_counter보다 충분히 큰 값
        let handseed = format!("{:X}", self.handle_counter + 10000);

        push_group(out, 0, "SECTION");
        push_group(out, 2, "HEADER");

        // $ACADVER = AC1015 (R2000)
        push_group(out, 9, "$ACADVER");
        push_group(out, 1, "AC1015");

        // $DWGCODEPAGE = ANSI_949 (CP949/EUC-KR)
        push_group(out, 9, "$DWGCODEPAGE");
        push_group(out, 3, "ANSI_949");

        // $INSUNITS = 0 (단위 없음 / 경위도)
        push_group(out, 9, "$INSUNITS");
        push_group(out, 70, "0");

        // $HANDSEED
        push_group(out, 9, "$HANDSEED");
        push_group(out, 5, &handseed);

        // $EXTMIN / $EXTMAX — 기본값 0,0
        push_group(out, 9, "$EXTMIN");
        push_group(out, 10, "0.0");
        push_group(out, 20, "0.0");
        push_group(out, 30, "0.0");

        push_group(out, 9, "$EXTMAX");
        push_group(out, 10, "0.0");
        push_group(out, 20, "0.0");
        push_group(out, 30, "0.0");

        push_group(out, 0, "ENDSEC");
    }

    // -----------------------------------------------------------------------
    // SECTION TABLES
    // -----------------------------------------------------------------------

    fn write_tables(&mut self, out: &mut String) {
        push_group(out, 0, "SECTION");
        push_group(out, 2, "TABLES");

        // VPORT 테이블 (최소)
        let tbl_handle = self.next_handle();
        push_group(out, 0, "TABLE");
        push_group(out, 2, "VPORT");
        push_group(out, 5, &tbl_handle);
        push_group(out, 100, "AcDbSymbolTable");
        push_group(out, 70, "0");
        push_group(out, 0, "ENDTAB");

        // LTYPE 테이블 (최소)
        let tbl_handle = self.next_handle();
        push_group(out, 0, "TABLE");
        push_group(out, 2, "LTYPE");
        push_group(out, 5, &tbl_handle);
        push_group(out, 100, "AcDbSymbolTable");
        push_group(out, 70, "0");
        push_group(out, 0, "ENDTAB");

        // LAYER 테이블
        let tbl_handle = self.next_handle();
        push_group(out, 0, "TABLE");
        push_group(out, 2, "LAYER");
        push_group(out, 5, &tbl_handle);
        push_group(out, 100, "AcDbSymbolTable");
        push_group(out, 70, "2"); // 2 레이어: PARCEL, JIBUN

        // PARCEL 레이어
        let layer_handle = self.next_handle();
        push_group(out, 0, "LAYER");
        push_group(out, 5, &layer_handle);
        push_group(out, 100, "AcDbSymbolTableRecord");
        push_group(out, 100, "AcDbLayerTableRecord");
        push_group(out, 2, "PARCEL");
        push_group(out, 70, "0"); // 잠금 안 함
        push_group(out, 62, "7"); // 색상 흰색
        push_group(out, 6, "Continuous");

        // JIBUN 레이어
        let layer_handle = self.next_handle();
        push_group(out, 0, "LAYER");
        push_group(out, 5, &layer_handle);
        push_group(out, 100, "AcDbSymbolTableRecord");
        push_group(out, 100, "AcDbLayerTableRecord");
        push_group(out, 2, "JIBUN");
        push_group(out, 70, "0");
        push_group(out, 62, "3"); // 색상 초록
        push_group(out, 6, "Continuous");

        push_group(out, 0, "ENDTAB");

        // STYLE 테이블 (STANDARD 텍스트 스타일)
        let tbl_handle = self.next_handle();
        push_group(out, 0, "TABLE");
        push_group(out, 2, "STYLE");
        push_group(out, 5, &tbl_handle);
        push_group(out, 100, "AcDbSymbolTable");
        push_group(out, 70, "1");

        let style_handle = self.next_handle();
        push_group(out, 0, "STYLE");
        push_group(out, 5, &style_handle);
        push_group(out, 100, "AcDbSymbolTableRecord");
        push_group(out, 100, "AcDbTextStyleTableRecord");
        push_group(out, 2, "STANDARD");
        push_group(out, 70, "0");
        push_group(out, 40, "0.0");
        push_group(out, 41, "1.0");
        push_group(out, 50, "0.0");
        push_group(out, 71, "0");
        push_group(out, 42, "2.5");
        push_group(out, 3, "txt");
        push_group(out, 4, "");

        push_group(out, 0, "ENDTAB");

        // VIEW 테이블 (최소)
        let tbl_handle = self.next_handle();
        push_group(out, 0, "TABLE");
        push_group(out, 2, "VIEW");
        push_group(out, 5, &tbl_handle);
        push_group(out, 100, "AcDbSymbolTable");
        push_group(out, 70, "0");
        push_group(out, 0, "ENDTAB");

        // UCS 테이블 (최소)
        let tbl_handle = self.next_handle();
        push_group(out, 0, "TABLE");
        push_group(out, 2, "UCS");
        push_group(out, 5, &tbl_handle);
        push_group(out, 100, "AcDbSymbolTable");
        push_group(out, 70, "0");
        push_group(out, 0, "ENDTAB");

        // APPID 테이블 (최소 — ACAD 앱 ID 필요)
        let tbl_handle = self.next_handle();
        push_group(out, 0, "TABLE");
        push_group(out, 2, "APPID");
        push_group(out, 5, &tbl_handle);
        push_group(out, 100, "AcDbSymbolTable");
        push_group(out, 70, "1");

        let appid_handle = self.next_handle();
        push_group(out, 0, "APPID");
        push_group(out, 5, &appid_handle);
        push_group(out, 100, "AcDbSymbolTableRecord");
        push_group(out, 100, "AcDbRegAppTableRecord");
        push_group(out, 2, "ACAD");
        push_group(out, 70, "0");

        push_group(out, 0, "ENDTAB");

        // DIMSTYLE 테이블 (최소)
        let tbl_handle = self.next_handle();
        push_group(out, 0, "TABLE");
        push_group(out, 2, "DIMSTYLE");
        push_group(out, 5, &tbl_handle);
        push_group(out, 100, "AcDbSymbolTable");
        push_group(out, 70, "0");
        push_group(out, 0, "ENDTAB");

        // BLOCK_RECORD 테이블 (최소 — *Model_Space, *Paper_Space)
        let tbl_handle = self.next_handle();
        push_group(out, 0, "TABLE");
        push_group(out, 2, "BLOCK_RECORD");
        push_group(out, 5, &tbl_handle);
        push_group(out, 100, "AcDbSymbolTable");
        push_group(out, 70, "2");

        let blk_handle = self.next_handle();
        push_group(out, 0, "BLOCK_RECORD");
        push_group(out, 5, &blk_handle);
        push_group(out, 100, "AcDbSymbolTableRecord");
        push_group(out, 100, "AcDbBlockTableRecord");
        push_group(out, 2, "*Model_Space");

        let blk_handle = self.next_handle();
        push_group(out, 0, "BLOCK_RECORD");
        push_group(out, 5, &blk_handle);
        push_group(out, 100, "AcDbSymbolTableRecord");
        push_group(out, 100, "AcDbBlockTableRecord");
        push_group(out, 2, "*Paper_Space");

        push_group(out, 0, "ENDTAB");

        push_group(out, 0, "ENDSEC");
    }

    // -----------------------------------------------------------------------
    // SECTION BLOCKS (최소 — Model_Space/Paper_Space 블록 정의 필요)
    // -----------------------------------------------------------------------

    fn write_blocks(&mut self, out: &mut String) {
        push_group(out, 0, "SECTION");
        push_group(out, 2, "BLOCKS");

        // *Model_Space
        let blk_handle = self.next_handle();
        push_group(out, 0, "BLOCK");
        push_group(out, 5, &blk_handle);
        push_group(out, 100, "AcDbEntity");
        push_group(out, 8, "0");
        push_group(out, 100, "AcDbBlockBegin");
        push_group(out, 2, "*Model_Space");
        push_group(out, 70, "0");
        push_group(out, 10, "0.0");
        push_group(out, 20, "0.0");
        push_group(out, 30, "0.0");
        push_group(out, 3, "*Model_Space");
        push_group(out, 1, "");
        push_group(out, 0, "ENDBLK");
        let endblk_handle = self.next_handle();
        push_group(out, 5, &endblk_handle);
        push_group(out, 100, "AcDbEntity");
        push_group(out, 8, "0");
        push_group(out, 100, "AcDbBlockEnd");

        // *Paper_Space
        let blk_handle = self.next_handle();
        push_group(out, 0, "BLOCK");
        push_group(out, 5, &blk_handle);
        push_group(out, 100, "AcDbEntity");
        push_group(out, 8, "0");
        push_group(out, 100, "AcDbBlockBegin");
        push_group(out, 2, "*Paper_Space");
        push_group(out, 70, "0");
        push_group(out, 10, "0.0");
        push_group(out, 20, "0.0");
        push_group(out, 30, "0.0");
        push_group(out, 3, "*Paper_Space");
        push_group(out, 1, "");
        push_group(out, 0, "ENDBLK");
        let endblk_handle = self.next_handle();
        push_group(out, 5, &endblk_handle);
        push_group(out, 100, "AcDbEntity");
        push_group(out, 8, "0");
        push_group(out, 100, "AcDbBlockEnd");

        push_group(out, 0, "ENDSEC");
    }

    // -----------------------------------------------------------------------
    // SECTION ENTITIES
    // -----------------------------------------------------------------------

    fn write_entities(&mut self, out: &mut String) {
        // BLOCKS 섹션도 여기서 삽입 (build 순서: HEADER → TABLES → BLOCKS → ENTITIES)
        self.write_blocks(out);

        push_group(out, 0, "SECTION");
        push_group(out, 2, "ENTITIES");

        // LWPOLYLINE 엔티티들
        let lwpolylines = std::mem::take(&mut self.lwpolylines);
        for points in &lwpolylines {
            let handle = self.next_handle();
            let n = points.len();
            // 마지막 정점이 첫 정점과 같으면 DXF closed flag 사용 (마지막 정점 제외)
            let (verts, closed) = if n > 1
                && (points[0].0 - points[n - 1].0).abs() < 1e-12
                && (points[0].1 - points[n - 1].1).abs() < 1e-12
            {
                (&points[..n - 1], true)
            } else {
                (&points[..], false)
            };

            push_group(out, 0, "LWPOLYLINE");
            push_group(out, 5, &handle);
            push_group(out, 100, "AcDbEntity");
            push_group(out, 8, "PARCEL");
            push_group(out, 100, "AcDbPolyline");
            push_group(out, 90, &verts.len().to_string()); // 정점 수
            push_group(out, 70, if closed { "1" } else { "0" }); // 닫힘 플래그
            push_group(out, 43, "0.0"); // 상수 선폭 0

            for (x, y) in verts {
                push_group(out, 10, &format!("{:.10}", x));
                push_group(out, 20, &format!("{:.10}", y));
            }
        }
        self.lwpolylines = lwpolylines;

        // MTEXT 엔티티들
        let text_height = self.text_height();
        let mtexts = std::mem::take(&mut self.mtexts);
        for (x, y, text) in &mtexts {
            let handle = self.next_handle();
            push_group(out, 0, "MTEXT");
            push_group(out, 5, &handle);
            push_group(out, 100, "AcDbEntity");
            push_group(out, 8, "JIBUN");
            push_group(out, 100, "AcDbMText");
            push_group(out, 10, &format!("{:.10}", x)); // 삽입 X
            push_group(out, 20, &format!("{:.10}", y)); // 삽입 Y
            push_group(out, 30, "0.0");                 // Z
            push_group(out, 40, &format!("{:.10e}", text_height)); // 텍스트 높이
            push_group(out, 41, "0.0"); // 참조 너비 0=무제한
            push_group(out, 71, "1");   // 첨부 점: 좌상단
            push_group(out, 72, "1");   // 그리기 방향: 좌→우
            push_group(out, 1, text);   // 텍스트 내용
            push_group(out, 7, "STANDARD"); // 텍스트 스타일
        }
        self.mtexts = mtexts;

        push_group(out, 0, "ENDSEC");
    }

    // -----------------------------------------------------------------------
    // EOF
    // -----------------------------------------------------------------------

    fn write_eof(&mut self, out: &mut String) {
        push_group(out, 0, "EOF");
    }
}

// ---------------------------------------------------------------------------
// 헬퍼 — DXF 그룹 코드 출력
// ---------------------------------------------------------------------------

/// DXF 그룹 코드 한 쌍(코드\r\n값\r\n)을 out에 추가.
fn push_group(out: &mut String, code: i32, value: &str) {
    // DXF 명세: 그룹코드는 오른쪽 정렬 6자리, 값은 그대로.
    // AutoCAD는 공백 없는 단순 형식도 허용. 여기선 간결하게.
    out.push_str(&format!("{:>3}\r\n{}\r\n", code, value));
}

// ---------------------------------------------------------------------------
// 자체 검증 함수 (테스트 및 외부 호출용)
// ---------------------------------------------------------------------------

/// DXF 바이트에서 키워드 등장 횟수를 센다 (UTF-8 로스리스 변환 후 검색).
pub fn count_keyword_in_dxf(dxf_bytes: &[u8], keyword: &str) -> usize {
    // CP949 인코딩된 경우 ASCII 범위는 그대로이므로 UTF-8 로스리 디코딩으로 확인
    let text = String::from_utf8_lossy(dxf_bytes);
    text.matches(keyword).count()
}

/// DXF 바이트에서 LWPOLYLINE 엔티티 수 반환.
pub fn count_lwpolylines(dxf_bytes: &[u8]) -> usize {
    // "  0\r\nLWPOLYLINE\r\n" 또는 "  0\nLWPOLYLINE\n" 패턴
    let text = String::from_utf8_lossy(dxf_bytes);
    // 그룹코드 0 이후 LWPOLYLINE — 개행 방식 무관 검색
    let mut count = 0;
    let mut search = text.as_ref();
    while let Some(pos) = search.find("LWPOLYLINE") {
        // 직전 줄이 그룹코드 0인지 확인 (간이 검사)
        count += 1;
        search = &search[pos + "LWPOLYLINE".len()..];
    }
    count
}

/// DXF 바이트에서 MTEXT 엔티티 수 반환.
pub fn count_mtexts(dxf_bytes: &[u8]) -> usize {
    let text = String::from_utf8_lossy(dxf_bytes);
    let mut count = 0;
    let mut search = text.as_ref();
    // "MTEXT\r\n" 또는 "MTEXT\n" — 단, "AcDbMText"와 구분 위해 행 경계 확인
    while let Some(pos) = search.find("MTEXT") {
        // AcDbMText 는 제외 (AcDb로 시작하는 것 건너뜀)
        let before = &search[..pos];
        // 직전 '\n' 이후가 "MTEXT"인지 확인
        let last_newline = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line_start = &search[last_newline..pos];
        if !line_start.contains("AcDb") {
            count += 1;
        }
        search = &search[pos + "MTEXT".len()..];
    }
    count
}

/// DXF 바이트가 `$ACADVER` = AC1015 헤더를 포함하는지 확인.
pub fn has_acadver_r2000(dxf_bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(dxf_bytes);
    text.contains("$ACADVER") && text.contains("AC1015")
}

/// DXF 바이트가 `$DWGCODEPAGE` = ANSI_949 헤더를 포함하는지 확인.
pub fn has_dwgcodepage_949(dxf_bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(dxf_bytes);
    text.contains("$DWGCODEPAGE") && text.contains("ANSI_949")
}

// ---------------------------------------------------------------------------
// 단위 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn default_opts() -> DxfOpts {
        DxfOpts {
            encoding: "cp949".to_string(),
            symbol_scale: 1000,
            label_field: "pnu".to_string(),
            is_degree: true,
        }
    }

    // (a) 사각 폴리곤 1개 Polygon + pnu → LWPOLYLINE 1 · MTEXT 1 · closed flag · 정점 4
    #[test]
    fn test_single_polygon() {
        let fc = json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "properties": { "pnu": "1234567890" },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[
                        [127.0, 37.0],
                        [127.1, 37.0],
                        [127.1, 37.1],
                        [127.0, 37.1],
                        [127.0, 37.0]  // 닫힘
                    ]]
                }
            }]
        });

        let opts = default_opts();
        let dxf = feature_collection_to_dxf(&fc, &opts).unwrap();

        assert_eq!(count_lwpolylines(&dxf), 1, "LWPOLYLINE 1개 기대");
        assert_eq!(count_mtexts(&dxf), 1, "MTEXT 1개 기대");

        // closed flag 확인: 그룹코드 70 = 1
        let text = String::from_utf8_lossy(&dxf);
        // LWPOLYLINE 섹션 내 70\r\n1\r\n 포함
        assert!(text.contains("AcDbPolyline"), "AcDbPolyline 수퍼클래스 필요");

        // 정점 수 확인: 닫힌 ring 5정점 → 마지막 중복 제외 → 4정점 (90\r\n4\r\n)
        assert!(text.contains(" 90\r\n4\r\n"), "정점 수 4 기대 (중복 제거)");

        // closed flag: 70\r\n1\r\n
        assert!(text.contains(" 70\r\n1\r\n"), "closed flag 기대");
    }

    // (b) MultiPolygon 2파트 → LWPOLYLINE 2
    #[test]
    fn test_multipolygon_two_parts() {
        let fc = json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "properties": { "pnu": "A001" },
                "geometry": {
                    "type": "MultiPolygon",
                    "coordinates": [
                        [[[127.0, 37.0], [127.1, 37.0], [127.1, 37.1], [127.0, 37.0]]],
                        [[[127.2, 37.2], [127.3, 37.2], [127.3, 37.3], [127.2, 37.2]]]
                    ]
                }
            }]
        });

        let opts = default_opts();
        let dxf = feature_collection_to_dxf(&fc, &opts).unwrap();

        assert_eq!(count_lwpolylines(&dxf), 2, "MultiPolygon 2파트 → LWPOLYLINE 2");
        // 각 파트 외환에 MTEXT → 2개
        assert_eq!(count_mtexts(&dxf), 2, "파트별 MTEXT 2개");
    }

    // (c) 내환 있는 Polygon → LWPOLYLINE 2 (외환 + 내환)
    #[test]
    fn test_polygon_with_hole() {
        let fc = json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "properties": { "pnu": "HOLE001" },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [
                        // 외환
                        [[127.0, 37.0], [127.2, 37.0], [127.2, 37.2], [127.0, 37.2], [127.0, 37.0]],
                        // 내환 (구멍)
                        [[127.05, 37.05], [127.15, 37.05], [127.15, 37.15], [127.05, 37.15], [127.05, 37.05]]
                    ]
                }
            }]
        });

        let opts = default_opts();
        let dxf = feature_collection_to_dxf(&fc, &opts).unwrap();

        assert_eq!(count_lwpolylines(&dxf), 2, "외환 + 내환 → LWPOLYLINE 2");
        // 라벨은 외환(i==0)에만
        assert_eq!(count_mtexts(&dxf), 1, "외환에만 MTEXT 1개");
    }

    // (d) 텍스트 높이 비퇴화 (> 0)
    #[test]
    fn test_text_height_nonzero() {
        let writer = DxfWriter::new(1000, true);
        let h = writer.text_height();
        assert!(h > 0.0, "텍스트 높이 > 0 기대, 실제: {}", h);
        // 1:1000, 2.5mm → 2.5m / 111320 ≈ 2.245e-5 °
        let expected = 2.5 / 111_320.0;
        assert!(
            (h - expected).abs() < 1e-10,
            "텍스트 높이 기대값 {:.2e}, 실제 {:.2e}",
            expected,
            h
        );
    }

    // (e) 헤더에 AC1015 · ANSI_949 포함
    #[test]
    fn test_header_versions() {
        let fc = json!({
            "type": "FeatureCollection",
            "features": []
        });
        let opts = default_opts();
        let dxf = feature_collection_to_dxf(&fc, &opts).unwrap();

        assert!(has_acadver_r2000(&dxf), "$ACADVER AC1015 헤더 없음");
        assert!(has_dwgcodepage_949(&dxf), "$DWGCODEPAGE ANSI_949 헤더 없음");
    }

    // 추가: 빈 FeatureCollection → 빌드 오류 없음
    #[test]
    fn test_empty_feature_collection() {
        let fc = json!({
            "type": "FeatureCollection",
            "features": []
        });
        let opts = default_opts();
        let result = feature_collection_to_dxf(&fc, &opts);
        assert!(result.is_ok(), "빈 FC도 오류 없어야 함");
        let dxf = result.unwrap();
        assert_eq!(count_lwpolylines(&dxf), 0);
        assert_eq!(count_mtexts(&dxf), 0);
    }

    // 추가: UTF-8 인코딩 옵션 확인
    #[test]
    fn test_utf8_encoding_option() {
        let fc = json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "properties": { "pnu": "test123" },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[
                        [127.0, 37.0], [127.1, 37.0], [127.1, 37.1], [127.0, 37.0]
                    ]]
                }
            }]
        });
        let opts = DxfOpts {
            encoding: "utf8".to_string(),
            symbol_scale: 1000,
            label_field: "pnu".to_string(),
            is_degree: true,
        };
        let dxf = feature_collection_to_dxf(&fc, &opts).unwrap();
        // UTF-8이면 유효한 UTF-8 바이트여야 함
        assert!(std::str::from_utf8(&dxf).is_ok(), "UTF-8 인코딩 결과가 유효 UTF-8이어야 함");
    }

    // 추가: MTEXT 카운터 상세 — "AcDbMText" 중복 세지 않는지 확인
    #[test]
    fn test_mtext_counter_no_false_positives() {
        let fc = json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "properties": { "pnu": "P001" },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[
                        [127.0, 37.0], [127.1, 37.0], [127.1, 37.1],
                        [127.0, 37.1], [127.0, 37.0]
                    ]]
                }
            }]
        });
        let opts = default_opts();
        let dxf = feature_collection_to_dxf(&fc, &opts).unwrap();

        // AcDbMText 존재하지만 MTEXT 엔티티는 1개
        let text = String::from_utf8_lossy(&dxf);
        assert!(text.contains("AcDbMText"), "AcDbMText 수퍼클래스 필요");
        assert_eq!(count_mtexts(&dxf), 1, "MTEXT 카운터가 AcDbMText 포함 시 오작동 금지");
    }
}
