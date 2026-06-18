//! 키 추가/제거/목록(마스킹)/검증 로직(설계 §4, ⑥).

use super::{Config, KeyEntry};
use anyhow::{anyhow, Result};

/// 키 마스킹 — 앞 4·뒤 4만 노출(예: `ABCD••••WXYZ`). 짧으면 전부 마스킹.
pub fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "•".repeat(chars.len().max(4));
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}••••{tail}")
}

/// 키 추가(중복 거부). 별칭·referer 선택.
pub fn add_key(
    cfg: &mut Config,
    key: String,
    alias: Option<String>,
    referer: Option<String>,
) -> Result<()> {
    if cfg.keys.iter().any(|k| k.key == key) {
        return Err(anyhow!("이미 등록된 키입니다: {}", mask(&key)));
    }
    cfg.keys.push(KeyEntry {
        key,
        alias,
        referer,
    });
    Ok(())
}

/// 키 제거 — 값 또는 0-기반 인덱스로 지정.
pub fn remove_key(cfg: &mut Config, target: &str) -> Result<KeyEntry> {
    // 인덱스 우선 시도.
    if let Ok(idx) = target.parse::<usize>() {
        if idx < cfg.keys.len() {
            return Ok(cfg.keys.remove(idx));
        }
        return Err(anyhow!(
            "인덱스 범위 초과: {idx} (등록 키 {}개)",
            cfg.keys.len()
        ));
    }
    // 값으로 제거.
    if let Some(pos) = cfg.keys.iter().position(|k| k.key == target) {
        return Ok(cfg.keys.remove(pos));
    }
    Err(anyhow!("일치하는 키를 찾을 수 없습니다: {}", mask(target)))
}

/// 마스킹된 목록(인덱스·별칭·referer 표시).
pub fn list_masked(cfg: &Config) -> Vec<MaskedKey> {
    cfg.keys
        .iter()
        .enumerate()
        .map(|(i, k)| MaskedKey {
            index: i,
            masked: mask(&k.key),
            alias: k.alias.clone(),
            referer: k.referer.clone(),
        })
        .collect()
}

/// 마스킹 목록 1행.
#[derive(Debug, serde::Serialize)]
pub struct MaskedKey {
    pub index: usize,
    pub masked: String,
    pub alias: Option<String>,
    pub referer: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_long_key() {
        assert_eq!(mask("ABCD1234-AB69-334B-A205-D4FAFC00WXYZ"), "ABCD••••WXYZ");
    }

    #[test]
    fn mask_short_key() {
        assert_eq!(mask("abc"), "••••");
        assert_eq!(mask("12345678"), "••••••••");
    }

    #[test]
    fn add_rejects_duplicate() {
        let mut cfg = Config::default();
        add_key(&mut cfg, "K1".into(), None, None).unwrap();
        assert!(add_key(&mut cfg, "K1".into(), None, None).is_err());
        assert_eq!(cfg.keys.len(), 1);
    }

    #[test]
    fn remove_by_index_and_value() {
        let mut cfg = Config::default();
        add_key(&mut cfg, "K1".into(), None, None).unwrap();
        add_key(&mut cfg, "K2".into(), None, None).unwrap();
        let removed = remove_key(&mut cfg, "0").unwrap();
        assert_eq!(removed.key, "K1");
        let removed = remove_key(&mut cfg, "K2").unwrap();
        assert_eq!(removed.key, "K2");
        assert!(cfg.keys.is_empty());
    }

    #[test]
    fn remove_missing_errors() {
        let mut cfg = Config::default();
        assert!(remove_key(&mut cfg, "nope").is_err());
        assert!(remove_key(&mut cfg, "5").is_err());
    }
}
