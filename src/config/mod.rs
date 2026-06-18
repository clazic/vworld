//! 설정·키 관리 — TOML, current_exe 기준 경로 자기완결 해석(설계 §4).

pub mod keys;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 설정파일 루트 스키마.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 전역 기본 referer(도메인 등록 키). 키별 referer가 우선.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,

    /// 등록된 API 키 목록 — 전체가 동시성 키 풀로 자동 편입.
    #[serde(default, rename = "keys")]
    pub keys: Vec<KeyEntry>,
}

/// 키 1건. 필드명은 `key`로 통일(serde rename 단일 출처).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEntry {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// 키 발급 시 등록한 도메인(Referer 헤더 및 `domain=` 쿼리로 주입).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
}

impl Config {
    /// 설정파일을 로드. 파일이 없으면 빈 설정 반환(키 0개).
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("설정파일 읽기 실패: {}", path.display()))?;
        let cfg: Config = toml::from_str(&text)
            .with_context(|| format!("설정파일 파싱 실패(TOML): {}", path.display()))?;
        Ok(cfg)
    }

    /// 설정파일을 저장(부모 디렉토리 자동 생성).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("설정 디렉토리 생성 실패: {}", parent.display()))?;
            }
        }
        let text = toml::to_string_pretty(self).context("설정 직렬화 실패")?;
        std::fs::write(path, text)
            .with_context(|| format!("설정파일 쓰기 실패: {}", path.display()))?;
        Ok(())
    }

    /// 특정 키의 referer(키별 > 전역 기본).
    pub fn referer_for(&self, entry: &KeyEntry) -> Option<String> {
        entry.referer.clone().or_else(|| self.referer.clone())
    }
}

/// config 경로 결정: `--config <path>` > `current_exe()/app/config.toml`(기본).
///
/// `--config` 지정 시 파일이 없으면 즉시 에러(기본 경로로 폴백하지 않음 — 설계 Step 1).
pub fn resolve_config_path(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        if !p.exists() {
            return Err(anyhow!(
                "--config 지정 경로가 존재하지 않습니다: {} (기본 경로로 폴백하지 않음)",
                p.display()
            ));
        }
        return Ok(p.to_path_buf());
    }
    default_config_path()
}

/// 기본 설정 경로 = 실행 바이너리와 **같은 디렉토리**의 `config.toml`.
///
/// 배포 구조상 바이너리(`<skill>/app/vworld`)와 설정(`<skill>/app/config.toml`)은 형제다.
/// 크로스플랫폼: `Path::join`만 사용, 슬래시 하드코딩 없음.
pub fn default_config_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("current_exe 해석 실패")?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow!("실행 바이너리 부모 디렉토리 없음"))?;
    Ok(dir.join("config.toml"))
}
