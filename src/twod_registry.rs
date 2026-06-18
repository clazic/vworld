//! 2D 데이터 레이어 레지스트리 — build.rs가 생성한 정적 테이블.

use serde::Serialize;

/// 2D 데이터 레이어 속성 1건(컴파일 타임 생성).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TwodAttr {
    pub name: &'static str,
    pub single_search: bool,
    pub r#type: &'static str,
    pub desc: &'static str,
}

/// 2D 데이터 레이어 1건(컴파일 타임 생성).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TwodLayer {
    /// 데이터ID (예: "LT_C_UQ111", "LP_PA_CBND_BUBUN")
    pub data_id: &'static str,
    /// svcIde (데이터ID와 별도 키 — 혼동 주의)
    pub svc_ide: &'static str,
    pub name: &'static str,
    pub cat: &'static str,
    /// 정규화된 geometry 타입: "Polygon" | "Line" | "Point"
    pub geom: &'static str,
    /// 속성 메타(build.rs가 twod_attrs.tsv에서 코드젠, 빈 배열 가능)
    pub attrs: &'static [TwodAttr],
}

include!(concat!(env!("OUT_DIR"), "/twod_catalog_gen.rs"));

/// 전체 레지스트리.
pub fn all() -> &'static [TwodLayer] {
    TWOD_LAYERS
}

/// 데이터ID로 검색(대소문자 무시 — 소문자 입력 허용).
pub fn find(data_id: &str) -> Option<&'static TwodLayer> {
    TWOD_LAYERS
        .iter()
        .find(|l| l.data_id.eq_ignore_ascii_case(data_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_158_entries() {
        assert_eq!(all().len(), 158, "2D 레이어 수가 158이 아님: {}", all().len());
    }

    #[test]
    fn attrs_one_to_one() {
        // 모든 레이어의 data_id가 유일해야 함 (카탈로그↔attrs 1:1은 build.rs가 보장,
        // 여기서는 런타임 중복 없음 확인)
        let mut seen = std::collections::BTreeSet::new();
        for layer in all() {
            assert!(
                seen.insert(layer.data_id),
                "중복 data_id 발견: {}",
                layer.data_id
            );
        }
    }

    #[test]
    fn golden_lt_c_uq111_six_attrs() {
        // LT_C_UQ111: 6속성 (uname, sido_name, sigg_name, dyear, dnum, ag_geom)
        let layer = find("LT_C_UQ111").expect("LT_C_UQ111 존재해야 함");
        assert_eq!(
            layer.attrs.len(),
            6,
            "LT_C_UQ111 속성 수가 6이 아님: {}",
            layer.attrs.len()
        );
        assert_eq!(layer.geom, "Polygon");
    }

    #[test]
    fn golden_lp_pa_cbnd_bubun_pnu_single_search() {
        // LP_PA_CBND_BUBUN: pnu 속성의 single_search = true
        let layer = find("LP_PA_CBND_BUBUN").expect("LP_PA_CBND_BUBUN 존재해야 함");
        let pnu = layer
            .attrs
            .iter()
            .find(|a| a.name == "pnu")
            .expect("pnu 속성 존재해야 함");
        assert!(pnu.single_search, "pnu.single_search가 true여야 함");
    }

    #[test]
    fn golden_lt_l_sprd_geom_line() {
        // LT_L_SPRD: geom = "Line"
        let layer = find("LT_L_SPRD").expect("LT_L_SPRD 존재해야 함");
        assert_eq!(layer.geom, "Line");
    }

    #[test]
    fn find_case_insensitive() {
        // 소문자 입력도 매칭돼야 함
        assert!(find("lt_c_uq111").is_some(), "소문자 입력 매칭 실패");
        assert!(find("nonexistent_layer").is_none());
    }

    #[test]
    fn empty_attrs_layer_ok() {
        // LT_P_WEISPLAFACW: 속성 0건 — 빈 슬라이스여야 함
        let layer = find("LT_P_WEISPLAFACW").expect("LT_P_WEISPLAFACW 존재해야 함");
        assert_eq!(layer.attrs.len(), 0, "LT_P_WEISPLAFACW attrs가 빈 슬라이스여야 함");
    }
}
