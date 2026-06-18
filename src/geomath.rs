//! 자기완결 지오 연산 — TM중부(EPSG:5186) 정변환 + point-in-polygon.
//!
//! PROJ C 의존 없이 직접 구현. 행정동 경계 SHP(EPSG:5186) 기반 필지 분류용.

/// EPSG:5186 (Korea 2000 / Central Belt 2010, GRS80 TM) 파라미터 — `.prj` 기준.
const A: f64 = 6_378_137.0; // GRS80 장반경
const F: f64 = 1.0 / 298.257_222_101; // 편평률
const LAT0_DEG: f64 = 38.0; // 원점 위도
const LON0_DEG: f64 = 127.0; // 중앙 자오선
const K0: f64 = 1.0; // 축척계수
const FE: f64 = 200_000.0; // False Easting
const FN: f64 = 600_000.0; // False Northing

/// 경위도(EPSG:4326, deg) → TM중부(EPSG:5186, m) 정변환. (E, N) 반환.
///
/// Snyder/USGS Transverse Mercator 급수식(이 위도대에서 mm급 정확도).
pub fn lonlat_to_tm5186(lon_deg: f64, lat_deg: f64) -> (f64, f64) {
    let e2 = F * (2.0 - F); // 제1이심률²
    let ep2 = e2 / (1.0 - e2);
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    let lat0 = LAT0_DEG.to_radians();
    let lon0 = LON0_DEG.to_radians();

    let n = A / (1.0 - e2 * lat.sin().powi(2)).sqrt();
    let t = lat.tan().powi(2);
    let c = ep2 * lat.cos().powi(2);
    let a_ = (lon - lon0) * lat.cos();

    let m = meridian_arc(lat, e2);
    let m0 = meridian_arc(lat0, e2);

    let east = FE
        + K0 * n
            * (a_ + (1.0 - t + c) * a_.powi(3) / 6.0
                + (5.0 - 18.0 * t + t * t + 72.0 * c - 58.0 * ep2) * a_.powi(5) / 120.0);
    let north = FN
        + K0 * (m - m0
            + n * lat.tan()
                * (a_.powi(2) / 2.0
                    + (5.0 - t + 9.0 * c + 4.0 * c * c) * a_.powi(4) / 24.0
                    + (61.0 - 58.0 * t + t * t + 600.0 * c - 330.0 * ep2) * a_.powi(6) / 720.0));
    (east, north)
}

/// 자오선호장 M(lat) — Snyder 급수.
fn meridian_arc(lat: f64, e2: f64) -> f64 {
    let e4 = e2 * e2;
    let e6 = e4 * e2;
    A * ((1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0) * lat
        - (3.0 * e2 / 8.0 + 3.0 * e4 / 32.0 + 45.0 * e6 / 1024.0) * (2.0 * lat).sin()
        + (15.0 * e4 / 256.0 + 45.0 * e6 / 1024.0) * (4.0 * lat).sin()
        - (35.0 * e6 / 3072.0) * (6.0 * lat).sin())
}

/// 축에 평행한 사각형(bbox).
#[derive(Debug, Clone, Copy)]
pub struct Bbox {
    pub minx: f64,
    pub miny: f64,
    pub maxx: f64,
    pub maxy: f64,
}

impl Bbox {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.minx && x <= self.maxx && y >= self.miny && y <= self.maxy
    }
    pub fn overlaps(&self, o: &Bbox) -> bool {
        self.minx <= o.maxx && self.maxx >= o.minx && self.miny <= o.maxy && self.maxy >= o.miny
    }
}

/// 점이 폴리곤(여러 링) 내부인지 — 모든 링에 대한 ray-casting 짝홀(even-odd) 규칙.
///
/// 외곽 링·구멍(hole)·다중 파트(섬)를 짝홀 패리티로 일관 처리(구멍 안=짝수=외부).
pub fn point_in_rings(px: f64, py: f64, rings: &[Vec<(f64, f64)>]) -> bool {
    let mut inside = false;
    for ring in rings {
        let n = ring.len();
        if n < 3 {
            continue;
        }
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = ring[i];
            let (xj, yj) = ring[j];
            if (yi > py) != (yj > py) {
                let x_cross = (xj - xi) * (py - yi) / (yj - yi) + xi;
                if px < x_cross {
                    inside = !inside;
                }
            }
            j = i;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tm_roundtrip_origin_region() {
        // 중앙자오선(127E) 위에서는 E ≈ FE(200000).
        let (e, _n) = lonlat_to_tm5186(127.0, 37.0);
        assert!((e - 200_000.0).abs() < 1.0, "E={e}");
        // 원점 위도(38N)에서 N ≈ FN(600000).
        let (_e2, n2) = lonlat_to_tm5186(127.0, 38.0);
        assert!((n2 - 600_000.0).abs() < 1.0, "N={n2}");
    }

    #[test]
    fn tm_known_point_seoul_range() {
        // 서울 시청 부근(126.978, 37.566) → 5186 좌표는 한국 TM중부 범위 내.
        let (e, n) = lonlat_to_tm5186(126.978, 37.566);
        assert!((150_000.0..250_000.0).contains(&e), "E={e}");
        assert!((540_000.0..560_000.0).contains(&n), "N={n}");
    }

    #[test]
    fn point_in_square() {
        let sq = vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]];
        assert!(point_in_rings(5.0, 5.0, &sq));
        assert!(!point_in_rings(15.0, 5.0, &sq));
        assert!(!point_in_rings(-1.0, 5.0, &sq));
    }

    #[test]
    fn point_in_polygon_with_hole() {
        // 외곽 0..10, 구멍 3..7. 구멍 안(5,5)은 외부, 링 사이(1,1)는 내부.
        let outer = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)];
        let hole = vec![(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0), (3.0, 3.0)];
        let rings = vec![outer, hole];
        assert!(!point_in_rings(5.0, 5.0, &rings), "구멍 안=외부");
        assert!(point_in_rings(1.0, 1.0, &rings), "링 사이=내부");
    }
}
