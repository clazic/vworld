//! 행정동 경계 SHP(EPSG:5186) 로더 + point-in-polygon 분류.
//!
//! VWorld 센서스 행정동경계(`BND_ADM_DONG_PG`)를 읽어, 필지점(EPSG:4326)을
//! TM중부로 변환 후 폴리곤 포함판정으로 행정동(ADM_NM)을 결정한다(역지오 불필요).

use crate::geomath::{lonlat_to_tm5186, point_in_rings, Bbox};
use anyhow::{anyhow, Context, Result};
use std::path::Path;

/// 행정동 폴리곤 1건(EPSG:5186 좌표).
pub struct DongPoly {
    pub name: String,
    pub cd: String,
    pub bbox: Bbox,
    pub rings: Vec<Vec<(f64, f64)>>,
}

/// 대상 영역과 겹치는 행정동 경계 집합(분류기).
pub struct HjdBoundaries {
    dongs: Vec<DongPoly>,
}

impl HjdBoundaries {
    /// 미리 준비된 폴리곤 목록으로 구성(DB 로드 경로용).
    pub fn from_dongs(dongs: Vec<DongPoly>) -> Self {
        HjdBoundaries { dongs }
    }

    /// `.shp` 경로에서 로드하되, `target`(EPSG:5186 bbox)과 겹치는 행정동만 보관.
    pub fn load_for_bbox(shp_path: &Path, target: Bbox) -> Result<Self> {
        let dongs: Vec<DongPoly> = read_all(shp_path)?
            .into_iter()
            .filter(|d| d.bbox.overlaps(&target))
            .collect();
        if dongs.is_empty() {
            return Err(anyhow!("대상 영역과 겹치는 행정동 폴리곤이 없습니다(좌표/경계 확인)"));
        }
        Ok(HjdBoundaries { dongs })
    }

    pub fn len(&self) -> usize {
        self.dongs.len()
    }

    /// EPSG:4326 점 → 포함 행정동명(없으면 None).
    pub fn classify_lonlat(&self, lon: f64, lat: f64) -> Option<&str> {
        let (x, y) = lonlat_to_tm5186(lon, lat);
        for d in &self.dongs {
            if d.bbox.contains(x, y) && point_in_rings(x, y, &d.rings) {
                return Some(&d.name);
            }
        }
        None
    }
}

/// SHP+DBF 전체를 읽어 행정동 폴리곤 목록 반환(bbox 필터 없음 — DB 적재용).
pub fn read_all(shp_path: &Path) -> Result<Vec<DongPoly>> {
    let attrs = read_adm_attrs(shp_path)?; // [(cd, nm)]
    let mut reader = shapefile::ShapeReader::from_path(shp_path)
        .with_context(|| format!("SHP 열기 실패: {}", shp_path.display()))?;

    let mut out = Vec::new();
    for (i, shape) in reader.iter_shapes().enumerate() {
        let polygon = match shape? {
            shapefile::Shape::Polygon(p) => p,
            _ => continue,
        };
        let mut rings: Vec<Vec<(f64, f64)>> = Vec::new();
        let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for ring in polygon.rings() {
            let pts: Vec<(f64, f64)> = ring
                .points()
                .iter()
                .map(|p| {
                    minx = minx.min(p.x);
                    miny = miny.min(p.y);
                    maxx = maxx.max(p.x);
                    maxy = maxy.max(p.y);
                    (p.x, p.y)
                })
                .collect();
            rings.push(pts);
        }
        if rings.is_empty() {
            continue;
        }
        let (cd, name) = attrs.get(i).cloned().unwrap_or_default();
        out.push(DongPoly {
            name,
            cd,
            bbox: Bbox { minx, miny, maxx, maxy },
            rings,
        });
    }
    if out.is_empty() {
        return Err(anyhow!("SHP에서 폴리곤을 읽지 못했습니다"));
    }
    Ok(out)
}

/// DBF(CP949/EUC-KR)에서 (ADM_CD, ADM_NM)을 레코드 순서대로 읽는다.
fn read_adm_attrs(shp_path: &Path) -> Result<Vec<(String, String)>> {
    let dbf_path = shp_path.with_extension("dbf");
    let bytes = std::fs::read(&dbf_path)
        .with_context(|| format!("DBF 읽기 실패: {}", dbf_path.display()))?;
    if bytes.len() < 32 {
        return Err(anyhow!("DBF 헤더 손상"));
    }
    let nrec = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let hlen = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    let rlen = u16::from_le_bytes([bytes[10], bytes[11]]) as usize;

    let mut off = 1usize;
    let mut cd_field = None;
    let mut nm_field = None;
    let mut p = 32usize;
    while p + 32 <= hlen {
        if bytes[p] == 0x0d {
            break;
        }
        let end = bytes[p..p + 11].iter().position(|&b| b == 0).unwrap_or(11);
        let fname = String::from_utf8_lossy(&bytes[p..p + end]);
        let flen = bytes[p + 16] as usize;
        match fname.as_ref() {
            "ADM_CD" => cd_field = Some((off, flen)),
            "ADM_NM" => nm_field = Some((off, flen)),
            _ => {}
        }
        off += flen;
        p += 32;
    }
    let nm = nm_field.ok_or_else(|| anyhow!("DBF에 ADM_NM 필드 없음"))?;

    let dec = |field: &[u8]| -> String {
        let (s, _, _) = encoding_rs::EUC_KR.decode(field);
        s.trim().to_string()
    };
    let mut out = Vec::with_capacity(nrec);
    for i in 0..nrec {
        let rec = hlen + i * rlen;
        if rec + rlen > bytes.len() {
            break;
        }
        let name = dec(&bytes[rec + nm.0..rec + nm.0 + nm.1]);
        let cd = cd_field
            .map(|(o, l)| dec(&bytes[rec + o..rec + o + l]))
            .unwrap_or_default();
        out.push((cd, name));
    }
    Ok(out)
}

/// 점 집합(EPSG:4326)의 bbox를 EPSG:5186으로 변환(폴리곤 사전필터용).
pub fn points_bbox_5186(points: &[(f64, f64)], pad_m: f64) -> Bbox {
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(lon, lat) in points {
        let (x, y) = lonlat_to_tm5186(lon, lat);
        minx = minx.min(x);
        miny = miny.min(y);
        maxx = maxx.max(x);
        maxy = maxy.max(y);
    }
    Bbox {
        minx: minx - pad_m,
        miny: miny - pad_m,
        maxx: maxx + pad_m,
        maxy: maxy + pad_m,
    }
}
