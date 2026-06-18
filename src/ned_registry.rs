//! NED 오퍼레이션 레지스트리 — build.rs가 생성한 정적 테이블(설계 §1.1.2·§8-⑩).

use serde::Serialize;

/// NED API 요청변수 1건(컴파일 타임 생성).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct NedParam {
    pub name: &'static str,
    pub required: bool,
    pub r#type: &'static str,
    pub default: &'static str,
    pub desc: &'static str,
}

/// NED 오퍼레이션 1건(컴파일 타임 생성).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct NedOp {
    pub apinum: u32,
    /// 계열: "wms" | "wfs" | "data".
    pub kind: &'static str,
    /// `/ned/{kind}/{endpoint_op}` 마지막 세그먼트.
    pub endpoint_op: &'static str,
    pub cat1: &'static str,
    pub cat2: &'static str,
    pub name: &'static str,
    /// 요청변수 메타(build.rs가 ned_params.tsv에서 코드젠).
    pub params: &'static [NedParam],
}

include!(concat!(env!("OUT_DIR"), "/ned_catalog_gen.rs"));

/// 전체 레지스트리.
pub fn all() -> &'static [NedOp] {
    NED_OPS
}

/// 오퍼레이션 이름(endpoint_op)으로 검색(대소문자 무시).
pub fn find(op: &str) -> Option<&'static NedOp> {
    NED_OPS
        .iter()
        .find(|o| o.endpoint_op.eq_ignore_ascii_case(op))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn registry_has_115_entries() {
        assert_eq!(all().len(), 115);
    }

    #[test]
    fn prefix_counts_match() {
        let mut c: BTreeMap<&str, usize> = BTreeMap::new();
        for op in all() {
            *c.entry(op.kind).or_default() += 1;
        }
        assert_eq!(c.get("wms").copied().unwrap_or(0), 36);
        assert_eq!(c.get("wfs").copied().unwrap_or(0), 33);
        assert_eq!(c.get("data").copied().unwrap_or(0), 46);
    }

    #[test]
    fn find_known_op() {
        let op = find("getBuildingAge").expect("getBuildingAge 존재");
        assert_eq!(op.kind, "data");
        assert!(find("getBuildingAgeWFS").is_some());
        assert!(find("nonexistent").is_none());
    }
}
