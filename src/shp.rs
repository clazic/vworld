//! GeoJSON FeatureCollection → ESRI Shapefile (.shp/.shx/.dbf/.prj/.cpg) 변환.
//!
//! VWorld 지적도 WFS 수집 결과(getCtnlgsSpceWFS 등)를 Shapefile로 내보낸다.
//! - shape 타입: Polygon(단순/다중파트)
//! - 속성(DBF): PNU, MNNM, SLNO, JIMOK(지목), EMD_CD(읍면동코드)
//! - 좌표계: .prj WKT 임베드(EPSG:4326/5185/5186/5187/5188)
//! - 인코딩: .cpg = UTF-8

use anyhow::{anyhow, Result};
use shapefile::dbase::{FieldName, FieldValue, Record, TableWriterBuilder};
use shapefile::{Point, Polygon, PolygonRing, Writer};
use std::path::Path;

// ───────────────────────── WKT 상수 ─────────────────────────

const PRJ_4326: &str =
    r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433],AUTHORITY["EPSG","4326"]]"#;

const PRJ_5185: &str =
    r#"PROJCS["Korea 2000 / West Belt 2010",GEOGCS["Korea 2000",DATUM["Korea_2000",SPHEROID["GRS 1980",6378137,298.257222101]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",38],PARAMETER["central_meridian",125],PARAMETER["scale_factor",1],PARAMETER["false_easting",200000],PARAMETER["false_northing",600000],UNIT["metre",1],AUTHORITY["EPSG","5185"]]"#;

const PRJ_5186: &str =
    r#"PROJCS["Korea 2000 / Central Belt 2010",GEOGCS["Korea 2000",DATUM["Korea_2000",SPHEROID["GRS 1980",6378137,298.257222101]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",38],PARAMETER["central_meridian",127],PARAMETER["scale_factor",1],PARAMETER["false_easting",200000],PARAMETER["false_northing",600000],UNIT["metre",1],AUTHORITY["EPSG","5186"]]"#;

const PRJ_5187: &str =
    r#"PROJCS["Korea 2000 / East Belt 2010",GEOGCS["Korea 2000",DATUM["Korea_2000",SPHEROID["GRS 1980",6378137,298.257222101]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",38],PARAMETER["central_meridian",129],PARAMETER["scale_factor",1],PARAMETER["false_easting",200000],PARAMETER["false_northing",600000],UNIT["metre",1],AUTHORITY["EPSG","5187"]]"#;

const PRJ_5188: &str =
    r#"PROJCS["Korea 2000 / East Sea Belt 2010",GEOGCS["Korea 2000",DATUM["Korea_2000",SPHEROID["GRS 1980",6378137,298.257222101]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",38],PARAMETER["central_meridian",131],PARAMETER["scale_factor",1],PARAMETER["false_easting",200000],PARAMETER["false_northing",600000],UNIT["metre",1],AUTHORITY["EPSG","5188"]]"#;

/// CRS 문자열 → WKT 문자열. 미정의면 None.
fn wkt_for_crs(crs: &str) -> Option<&'static str> {
    match crs.to_uppercase().as_str() {
        "EPSG:4326" => Some(PRJ_4326),
        "EPSG:5185" => Some(PRJ_5185),
        "EPSG:5186" => Some(PRJ_5186),
        "EPSG:5187" => Some(PRJ_5187),
        "EPSG:5188" => Some(PRJ_5188),
        _ => None,
    }
}

// ───────────────────────── GeoJSON 좌표 파싱 ─────────────────────────

/// GeoJSON 좌표 쌍([x, y]) → Point.
fn as_point(v: &serde_json::Value) -> Option<Point> {
    let a = v.as_array()?;
    if a.len() >= 2 {
        Some(Point::new(a[0].as_f64()?, a[1].as_f64()?))
    } else {
        None
    }
}

/// 좌표 링 → Vec<Point>. 닫히지 않았으면 첫 점 추가해 닫음.
fn parse_ring(ring_arr: &[serde_json::Value]) -> Option<Vec<Point>> {
    let pts: Vec<Point> = ring_arr.iter().filter_map(as_point).collect();
    if pts.len() < 4 {
        return None; // 최소 3정점 + 닫힘점
    }
    Some(pts)
}

/// GeoJSON Polygon 좌표(`[[ring], [hole], ...]`) → PolygonRing 목록.
/// GeoJSON: 외곽 CCW / 내환 CW (RFC 7946)
/// shapefile: `PolygonRing::Outer` / `PolygonRing::Inner` — 크레이트가 방향 조정.
fn polygon_rings_from_geojson(
    coords: &serde_json::Value,
) -> Option<Vec<PolygonRing<Point>>> {
    let rings_arr = coords.as_array()?;
    let mut rings: Vec<PolygonRing<Point>> = Vec::new();
    for (i, ring_val) in rings_arr.iter().enumerate() {
        let ring_arr = ring_val.as_array()?;
        let pts = parse_ring(ring_arr)?;
        if i == 0 {
            rings.push(PolygonRing::Outer(pts));
        } else {
            rings.push(PolygonRing::Inner(pts));
        }
    }
    if rings.is_empty() {
        return None;
    }
    Some(rings)
}

/// GeoJSON feature 하나의 geometry → PolygonRing 목록(다중파트 지원).
/// Polygon: 그대로. MultiPolygon: 모든 파트 링을 평탄화.
fn geometry_to_rings(geom: &serde_json::Value) -> Option<Vec<PolygonRing<Point>>> {
    let ty = geom.get("type")?.as_str()?;
    let coords = geom.get("coordinates")?;
    match ty {
        "Polygon" => polygon_rings_from_geojson(coords),
        "MultiPolygon" => {
            // coords: [ polygon_coords, ... ]
            let polys = coords.as_array()?;
            let mut all: Vec<PolygonRing<Point>> = Vec::new();
            for poly_coords in polys {
                if let Some(mut rings) = polygon_rings_from_geojson(poly_coords) {
                    all.append(&mut rings);
                }
            }
            if all.is_empty() { None } else { Some(all) }
        }
        _ => None,
    }
}

// ───────────────────────── 속성 헬퍼 ─────────────────────────

/// properties Value에서 문자열 값 추출(없으면 빈 문자열). 최대 len 바이트로 자름.
fn prop_str(props: &serde_json::Value, key: &str, max_len: usize) -> String {
    let s = match props.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(v) if !v.is_null() => v.to_string(),
        _ => String::new(),
    };
    // UTF-8 바이트 기준 자름(문자 경계 안전).
    if s.len() <= max_len {
        s
    } else {
        let mut end = max_len;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }
}

// ───────────────────────── 공개 API ─────────────────────────

/// GeoJSON FeatureCollection → Shapefile 5종(.shp/.shx/.dbf/.prj/.cpg).
///
/// `path`는 `.shp` 경로. `.shx`/`.dbf`/`.prj`/`.cpg`는 같은 basename으로 자동 생성.
pub fn feature_collection_to_shp(
    fc: &serde_json::Value,
    path: &Path,
    crs: &str,
) -> Result<usize> {
    let features = fc
        .get("features")
        .and_then(|f| f.as_array())
        .ok_or_else(|| anyhow!("FeatureCollection.features 없음"))?;

    // ── DBF 스키마 정의 ──
    // 필드명 최대 10자(DBF 규격), 길이는 바이트 단위(Character).
    let table_builder = TableWriterBuilder::new()
        .add_character_field(FieldName::try_from("PNU").unwrap(), 19)
        .add_character_field(FieldName::try_from("MNNM").unwrap(), 10)
        .add_character_field(FieldName::try_from("SLNO").unwrap(), 10)
        .add_character_field(FieldName::try_from("JIMOK").unwrap(), 4)
        .add_character_field(FieldName::try_from("EMD_CD").unwrap(), 10);

    let mut writer = Writer::from_path(path, table_builder)
        .map_err(|e| anyhow!("Shapefile 생성 실패: {e}"))?;

    let mut written = 0usize;
    let empty_props = serde_json::Value::Object(Default::default());

    for feat in features {
        let geom = match feat.get("geometry") {
            Some(g) if !g.is_null() => g,
            _ => continue,
        };
        let rings = match geometry_to_rings(geom) {
            Some(r) => r,
            None => continue,
        };

        let props = feat.get("properties").unwrap_or(&empty_props);

        let polygon = Polygon::with_rings(rings);

        // DBF 레코드
        let mut record = Record::default();
        record.insert("PNU".to_string(), FieldValue::Character(Some(prop_str(props, "pnu", 19))));
        record.insert("MNNM".to_string(), FieldValue::Character(Some(prop_str(props, "mnnm", 10))));
        record.insert("SLNO".to_string(), FieldValue::Character(Some(prop_str(props, "slno", 10))));
        record.insert("JIMOK".to_string(), FieldValue::Character(Some(prop_str(props, "lnm_lndcgr_smbol", 4))));
        record.insert("EMD_CD".to_string(), FieldValue::Character(Some(prop_str(props, "ld_emd_li_code", 10))));

        writer
            .write_shape_and_record(&polygon, &record)
            .map_err(|e| anyhow!("shape/record 쓰기 실패: {e}"))?;
        written += 1;
    }

    drop(writer); // 파일 flush + 헤더 업데이트

    // ── .prj 작성 ──
    let prj_path = path.with_extension("prj");
    match wkt_for_crs(crs) {
        Some(wkt) => {
            std::fs::write(&prj_path, wkt)
                .map_err(|e| anyhow!(".prj 쓰기 실패: {e}"))?;
        }
        None => {
            eprintln!("[경고] 알 수 없는 CRS '{crs}' — .prj 파일을 생성하지 않습니다.");
        }
    }

    // ── .cpg 작성 (UTF-8) ──
    let cpg_path = path.with_extension("cpg");
    std::fs::write(&cpg_path, "UTF-8")
        .map_err(|e| anyhow!(".cpg 쓰기 실패: {e}"))?;

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shp_roundtrip_polygon_and_multipolygon() {
        let fc = serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": {
                        "type": "Polygon",
                        "coordinates": [
                            [[129.1,35.1],[129.2,35.1],[129.2,35.2],[129.1,35.2],[129.1,35.1]]
                        ]
                    },
                    "properties": {
                        "pnu": "2644010100100010000",
                        "mnnm": "1",
                        "slno": "0",
                        "lnm_lndcgr_smbol": "전",
                        "ld_emd_li_code": "2644010100"
                    }
                },
                {
                    "type": "Feature",
                    "geometry": {
                        "type": "MultiPolygon",
                        "coordinates": [
                            [[[129.3,35.3],[129.4,35.3],[129.4,35.4],[129.3,35.4],[129.3,35.3]]]
                        ]
                    },
                    "properties": {
                        "pnu": "2644010100200020000",
                        "mnnm": "2",
                        "slno": "0",
                        "lnm_lndcgr_smbol": "답",
                        "ld_emd_li_code": "2644010100"
                    }
                }
            ]
        });

        let tmp = std::env::temp_dir().join("vworld_shp_test.shp");
        let count = feature_collection_to_shp(&fc, &tmp, "EPSG:5187").unwrap();
        assert_eq!(count, 2, "2개 feature가 써져야 함");

        // .shp/.shx/.dbf/.prj/.cpg 존재 확인
        assert!(tmp.exists(), ".shp 없음");
        assert!(tmp.with_extension("shx").exists(), ".shx 없음");
        assert!(tmp.with_extension("dbf").exists(), ".dbf 없음");
        assert!(tmp.with_extension("prj").exists(), ".prj 없음");
        assert!(tmp.with_extension("cpg").exists(), ".cpg 없음");

        // .prj 내용에 5187/East Belt 포함
        let prj = std::fs::read_to_string(tmp.with_extension("prj")).unwrap();
        assert!(prj.contains("5187") || prj.contains("East Belt"), ".prj에 5187 없음: {prj}");

        // .cpg = UTF-8
        let cpg = std::fs::read_to_string(tmp.with_extension("cpg")).unwrap();
        assert_eq!(cpg.trim(), "UTF-8");

        // shapefile::Reader로 재읽기 — polygon 수 및 dbf 레코드 수 일치
        let mut reader = shapefile::Reader::from_path(&tmp).unwrap();
        let mut poly_count = 0usize;
        for result in reader.iter_shapes_and_records() {
            let (shape, record) = result.unwrap();
            match shape {
                shapefile::Shape::Polygon(_) => poly_count += 1,
                _ => panic!("Polygon 외 shape 타입"),
            }
            // PNU 필드 존재 확인
            assert!(record.get("PNU").is_some(), "PNU 필드 없음");
        }
        assert_eq!(poly_count, 2, "재읽기 polygon 수 불일치");

        // 정리
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(tmp.with_extension("shx"));
        let _ = std::fs::remove_file(tmp.with_extension("dbf"));
        let _ = std::fs::remove_file(tmp.with_extension("prj"));
        let _ = std::fs::remove_file(tmp.with_extension("cpg"));
    }

    #[test]
    fn test_prj_4326() {
        let fc = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[126.9,37.5],[127.0,37.5],[127.0,37.6],[126.9,37.6],[126.9,37.5]]]
                },
                "properties": {}
            }]
        });
        let tmp = std::env::temp_dir().join("vworld_shp_4326.shp");
        feature_collection_to_shp(&fc, &tmp, "EPSG:4326").unwrap();
        let prj = std::fs::read_to_string(tmp.with_extension("prj")).unwrap();
        assert!(prj.contains("WGS"), ".prj에 WGS84 없음");
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(tmp.with_extension("shx"));
        let _ = std::fs::remove_file(tmp.with_extension("dbf"));
        let _ = std::fs::remove_file(tmp.with_extension("prj"));
        let _ = std::fs::remove_file(tmp.with_extension("cpg"));
    }
}
