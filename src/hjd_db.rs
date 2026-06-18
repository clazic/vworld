//! 행정동 경계 SQLite 적재/조회.
//!
//! SHP(EPSG:5186)를 한 번 적재해두면 이후 `--hjd-db`로 빠르게 재사용한다(129MB SHP 재파싱 불필요).
//! 폴리곤 링은 바이너리 blob으로 저장, bbox 컬럼에 인덱스를 걸어 영역 질의를 가속.

use crate::geomath::Bbox;
use crate::hjd_shp::{read_all, DongPoly, HjdBoundaries};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;

/// 링 목록 → blob: u32 nrings, [u32 npts, npts*(f64 x, f64 y)].
fn rings_to_blob(rings: &[Vec<(f64, f64)>]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&(rings.len() as u32).to_le_bytes());
    for ring in rings {
        b.extend_from_slice(&(ring.len() as u32).to_le_bytes());
        for &(x, y) in ring {
            b.extend_from_slice(&x.to_le_bytes());
            b.extend_from_slice(&y.to_le_bytes());
        }
    }
    b
}

/// blob → 링 목록.
fn blob_to_rings(b: &[u8]) -> Vec<Vec<(f64, f64)>> {
    let mut rings = Vec::new();
    let mut p = 0usize;
    let rd_u32 = |b: &[u8], p: usize| u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]);
    let rd_f64 = |b: &[u8], p: usize| {
        f64::from_le_bytes([
            b[p], b[p + 1], b[p + 2], b[p + 3], b[p + 4], b[p + 5], b[p + 6], b[p + 7],
        ])
    };
    if b.len() < 4 {
        return rings;
    }
    let nrings = rd_u32(b, p);
    p += 4;
    for _ in 0..nrings {
        if p + 4 > b.len() {
            break;
        }
        let npts = rd_u32(b, p) as usize;
        p += 4;
        let mut ring = Vec::with_capacity(npts);
        for _ in 0..npts {
            if p + 16 > b.len() {
                break;
            }
            ring.push((rd_f64(b, p), rd_f64(b, p + 8)));
            p += 16;
        }
        rings.push(ring);
    }
    rings
}

/// SHP를 읽어 SQLite DB에 적재(전국 행정동 전부). 기존 테이블은 재생성.
pub fn build_from_shp(shp_path: &Path, db_path: &Path) -> Result<usize> {
    let dongs = read_all(shp_path)?;
    let conn = Connection::open(db_path)
        .with_context(|| format!("DB 열기 실패: {}", db_path.display()))?;
    conn.execute_batch(
        "PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
         DROP TABLE IF EXISTS hjd;
         CREATE TABLE hjd(
            id INTEGER PRIMARY KEY,
            adm_nm TEXT NOT NULL,
            adm_cd TEXT,
            minx REAL, miny REAL, maxx REAL, maxy REAL,
            geom BLOB NOT NULL
         );",
    )?;
    {
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO hjd(adm_nm,adm_cd,minx,miny,maxx,maxy,geom) VALUES(?,?,?,?,?,?,?)",
            )?;
            for d in &dongs {
                stmt.execute(params![
                    d.name,
                    d.cd,
                    d.bbox.minx,
                    d.bbox.miny,
                    d.bbox.maxx,
                    d.bbox.maxy,
                    rings_to_blob(&d.rings),
                ])?;
            }
        }
        tx.commit()?;
    }
    // bbox 영역질의 가속용 인덱스.
    conn.execute_batch("CREATE INDEX idx_hjd_bbox ON hjd(minx,maxx,miny,maxy);")?;
    Ok(dongs.len())
}

/// calamine Data → 정리된 문자열(숫자 셀은 정수화).
fn cell_str(d: &calamine::Data) -> String {
    use calamine::Data;
    match d {
        Data::String(s) => s.trim().to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 {
                (*f as i64).to_string()
            } else {
                f.to_string()
            }
        }
        _ => String::new(),
    }
}

/// 센서스 지역코드 xlsx(시도/시군구/읍면동) → `region_code` 테이블 적재.
///
/// 행정동코드(ADM_CD, 8자리) = 시도(2)+시군구(3)+읍면동(3)으로 zero-pad해 `hjd.adm_cd`와 조인 가능.
/// 기본 시트는 SHP 기준일에 맞는 최신본. 컬럼: 시도코드,시도명,시군구코드,시군구명,읍면동코드,읍면동명.
pub fn build_region_from_xlsx(xlsx_path: &Path, sheet: &str, db_path: &Path) -> Result<usize> {
    use calamine::{open_workbook, Reader, Xlsx};
    let mut wb: Xlsx<_> =
        open_workbook(xlsx_path).with_context(|| format!("xlsx 열기 실패: {}", xlsx_path.display()))?;
    let range = wb
        .worksheet_range(sheet)
        .with_context(|| format!("시트 '{sheet}' 없음"))?;

    let conn = Connection::open(db_path)
        .with_context(|| format!("DB 열기 실패: {}", db_path.display()))?;
    conn.execute_batch(
        "DROP TABLE IF EXISTS region_code;
         CREATE TABLE region_code(
            adm_cd TEXT PRIMARY KEY,
            sido_cd TEXT, sido_nm TEXT,
            sgg_cd TEXT,  sgg_nm TEXT,
            emd_cd TEXT,  emd_nm TEXT,
            period TEXT
         );",
    )?;

    let mut n = 0usize;
    {
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO region_code(adm_cd,sido_cd,sido_nm,sgg_cd,sgg_nm,emd_cd,emd_nm,period)
                 VALUES(?,?,?,?,?,?,?,?)",
            )?;
            for row in range.rows() {
                if row.len() < 6 {
                    continue;
                }
                let sido = cell_str(&row[0]);
                let sido_nm = cell_str(&row[1]);
                let sgg = cell_str(&row[2]);
                let sgg_nm = cell_str(&row[3]);
                let emd = cell_str(&row[4]);
                let emd_nm = cell_str(&row[5]);
                // 헤더/제목/빈행 스킵: 시도코드가 2자리 숫자가 아니면 건너뜀.
                if sido.len() != 2 || !sido.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let adm_cd = format!("{:0>2}{:0>3}{:0>3}", sido, sgg.trim(), emd);
                stmt.execute(params![adm_cd, sido, sido_nm, sgg, sgg_nm, emd, emd_nm, sheet])?;
                n += 1;
            }
        }
        tx.commit()?;
    }
    Ok(n)
}

/// DB에서 `target`(EPSG:5186 bbox)과 겹치는 행정동만 로드.
pub fn load_for_bbox(db_path: &Path, target: Bbox) -> Result<HjdBoundaries> {
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .with_context(|| format!("DB 열기 실패: {}", db_path.display()))?;
    // bbox 겹침: 행정동.minx<=대상.maxx AND .maxx>=대상.minx AND .miny<=대상.maxy AND .maxy>=대상.miny
    let mut stmt = conn.prepare(
        "SELECT adm_nm,adm_cd,minx,miny,maxx,maxy,geom FROM hjd
         WHERE minx<=?1 AND maxx>=?2 AND miny<=?3 AND maxy>=?4",
    )?;
    let rows = stmt.query_map(
        params![target.maxx, target.minx, target.maxy, target.miny],
        |r| {
            let name: String = r.get(0)?;
            let cd: String = r.get(1)?;
            let minx: f64 = r.get(2)?;
            let miny: f64 = r.get(3)?;
            let maxx: f64 = r.get(4)?;
            let maxy: f64 = r.get(5)?;
            let geom: Vec<u8> = r.get(6)?;
            Ok(DongPoly {
                name,
                cd,
                bbox: Bbox { minx, miny, maxx, maxy },
                rings: blob_to_rings(&geom),
            })
        },
    )?;
    let dongs: Vec<DongPoly> = rows.collect::<rusqlite::Result<_>>()?;
    Ok(HjdBoundaries::from_dongs(dongs))
}

/// ADM_CD(8자리) 또는 동명 일부로 경계(hjd) + 지역코드(region_code)를 조인 조회.
pub fn lookup(db_path: &Path, query: &str) -> Result<Vec<serde_json::Value>> {
    let conn = Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("DB 열기 실패: {}", db_path.display()))?;
    // hjd.adm_cd ↔ region_code.adm_cd 조인. 코드면 정확 일치, 아니면 동명 LIKE.
    let is_code = query.chars().all(|c| c.is_ascii_digit()) && !query.is_empty();
    let sql = "SELECT h.adm_cd, h.adm_nm, r.sido_nm, r.sgg_nm, r.emd_nm
               FROM hjd h LEFT JOIN region_code r ON h.adm_cd = r.adm_cd
               WHERE (?1=1 AND h.adm_cd=?2) OR (?1=0 AND h.adm_nm LIKE ?3)
               ORDER BY h.adm_cd LIMIT 200";
    let mut stmt = conn.prepare(sql)?;
    let like = format!("%{query}%");
    let rows = stmt.query_map(
        params![if is_code { 1 } else { 0 }, query, like],
        |r| {
            Ok(serde_json::json!({
                "adm_cd": r.get::<_, String>(0)?,
                "adm_nm": r.get::<_, String>(1)?,
                "시도": r.get::<_, Option<String>>(2)?,
                "시군구": r.get::<_, Option<String>>(3)?,
                "읍면동": r.get::<_, Option<String>>(4)?,
            }))
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_roundtrip() {
        let rings = vec![
            vec![(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)],
            vec![(7.5, 8.25)],
        ];
        let b = rings_to_blob(&rings);
        let back = blob_to_rings(&b);
        assert_eq!(back, rings);
    }
}
