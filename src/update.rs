//! 자가 업데이트 — GitHub Releases 기반 self-update 및 주기적 버전 감지.
//!
//! - `run_self_update()`: 실제 바이너리 교체 (vworld update 명령)
//! - `fetch_latest_tag()`: 최신 릴리즈 태그 조회만
//! - `maybe_notify()`: 하루 1회 체크 후 새 버전이 있으면 stderr 알림(자동 교체 안 함)

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REPO_OWNER: &str = "clazic";
const REPO_NAME: &str = "vworld";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// 업데이트 체크 캐시 만료 시간 (24시간).
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

// ─────────────────────────── OS 타깃 매핑 ────────────────────────────────────

/// 현재 플랫폼에 맞는 자산명 키워드 반환.
/// self_update의 `asset_for(target)`는 자산명이 target을 **부분 포함**하면 매칭.
/// release.yml 자산: vworld-macos / vworld-linux / vworld-windows.exe / vworld(Linux fallback).
pub fn os_target() -> Result<&'static str> {
    #[cfg(target_os = "macos")]
    return Ok("macos");

    #[cfg(target_os = "linux")]
    return Ok("linux");

    #[cfg(target_os = "windows")]
    return Ok("windows");

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return Err(anyhow::anyhow!(
        "지원하지 않는 플랫폼입니다. macOS / Linux / Windows만 지원합니다."
    ));
}

// ─────────────────────────── 버전 정규화 ─────────────────────────────────────

/// "v1.2.3" → "1.2.3" 정규화(self_update 내장 비교에 전달 전 방어적 처리).
pub fn strip_v(version: &str) -> &str {
    version.trim_start_matches('v')
}

/// `latest` > `current`이면 true.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let l = strip_v(latest);
    let c = strip_v(current);
    // self_update 내장 version 비교 헬퍼 재사용.
    self_update::version::bump_is_greater(c, l).unwrap_or(false)
}

// ─────────────────────────── 최신 태그 조회 ──────────────────────────────────

/// GitHub Releases에서 최신 버전 태그를 가져온다(blocking).
/// spawn_blocking 안에서 호출할 것.
pub fn fetch_latest_tag() -> Result<String> {
    let release = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name("vworld")
        .current_version(CURRENT_VERSION)
        .build()
        .context("self_update 빌더 설정 실패")?
        .get_latest_release()
        .context("최신 릴리즈 조회 실패")?;
    Ok(release.version)
}

// ─────────────────────────── 실제 업데이트 ───────────────────────────────────

/// 자산을 내려받아 현재 바이너리를 교체한다(blocking).
/// spawn_blocking 안에서 호출할 것.
pub fn run_self_update(
    target: &str,
    version_tag: Option<&str>,
    no_confirm: bool,
) -> Result<self_update::Status> {
    let mut builder = self_update::backends::github::Update::configure();
    builder
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name("vworld")
        // Windows는 EXE_SUFFIX(.exe)를 자동 부착하므로 OS 분기 불필요.
        .target(target)
        .current_version(CURRENT_VERSION)
        .show_download_progress(true)
        .no_confirm(no_confirm);

    if let Some(tag) = version_tag {
        builder.target_version_tag(tag);
    }

    let status = builder
        .build()
        .context("self_update 빌더 설정 실패")?
        .update()
        .context("바이너리 업데이트 실패")?;

    Ok(status)
}

// ─────────────────────────── 캐시 경로 ───────────────────────────────────────

/// 업데이트 체크 캐시 파일 경로: `~/.vworld/.update-check`.
pub fn cache_path() -> Result<PathBuf> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");

    let home = home
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| anyhow::anyhow!("홈 디렉토리를 확인할 수 없습니다"))?;

    Ok(home.join(".vworld").join(".update-check"))
}

/// 캐시에서 (last_check_epoch, latest_tag) 읽기. 없거나 파싱 실패 시 None.
fn read_cache(path: &PathBuf) -> Option<(u64, String)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let epoch: u64 = lines.next()?.trim().parse().ok()?;
    let tag = lines.next()?.trim().to_string();
    if tag.is_empty() {
        return None;
    }
    Some((epoch, tag))
}

/// 캐시 쓰기. 실패해도 조용히 무시.
fn write_cache(path: &PathBuf, latest_tag: &str) {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(path));
    let _ = std::fs::write(path, format!("{epoch}\n{latest_tag}\n"));
}

/// 현재 epoch(초).
fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

// ─────────────────────────── 자동 알림 (best-effort) ─────────────────────────

/// 하루 1회 최신 버전 체크 후 새 버전이 있으면 stderr에 알림만 출력.
/// 자동 다운로드·교체는 하지 않는다.
/// 모든 실패는 조용히 무시(오프라인·rate-limit 등).
pub fn maybe_notify() {
    // CI 환경 또는 사용자가 명시적으로 끈 경우 생략.
    if std::env::var_os("CI").is_some()
        || std::env::var_os("VWORLD_NO_UPDATE_CHECK").is_some()
    {
        return;
    }

    let Ok(cache) = cache_path() else { return };

    // 24시간 게이트: 아직 만료 안 됐으면 캐시된 태그로만 비교.
    let now = now_epoch();
    if let Some((last_check, latest_tag)) = read_cache(&cache) {
        if now.saturating_sub(last_check) < CHECK_INTERVAL_SECS {
            // 캐시가 유효한 경우 — 네트워크 없이 비교만.
            if is_newer(&latest_tag, CURRENT_VERSION) {
                eprintln!(
                    "\n[vworld] 새 버전 {latest_tag} 이 있습니다. \
                    `vworld update` 를 실행하여 업데이트하세요."
                );
            }
            return;
        }
    }

    // 캐시 만료 → GitHub API 조회(동기 blocking, 별도 스레드 없이 best-effort).
    // main 종료 직전 호출이므로 스레드 낭비 최소화.
    let latest_tag = match fetch_latest_tag() {
        Ok(t) => t,
        Err(_) => return, // 네트워크 실패 — 조용히 무시.
    };

    write_cache(&cache, &latest_tag);

    if is_newer(&latest_tag, CURRENT_VERSION) {
        eprintln!(
            "\n[vworld] 새 버전 {latest_tag} 이 있습니다. \
            `vworld update` 를 실행하여 업데이트하세요."
        );
    }
}

// ─────────────────────────── 단위 테스트 ─────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_v() {
        assert_eq!(strip_v("v1.2.3"), "1.2.3");
        assert_eq!(strip_v("1.2.3"), "1.2.3");
        assert_eq!(strip_v("v0.1.0"), "0.1.0");
    }

    #[test]
    fn test_is_newer() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
    }

    #[test]
    fn test_os_target_valid() {
        // 현재 플랫폼에서 os_target()은 Ok여야 함(macOS/Linux/Windows 중 하나).
        let result = os_target();
        assert!(result.is_ok(), "os_target() 실패: {:?}", result);
        let t = result.unwrap();
        assert!(
            ["macos", "linux", "windows"].contains(&t),
            "예상치 못한 타깃: {t}"
        );
    }

    #[test]
    fn test_cache_rw(  ) {
        let dir = std::env::temp_dir().join(format!("vworld-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".update-check");

        // 없는 경우 None.
        assert!(read_cache(&path).is_none());

        // 쓰기 후 읽기.
        write_cache(&path, "v0.9.9");
        let (_, tag) = read_cache(&path).unwrap();
        assert_eq!(strip_v(&tag), "0.9.9");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cache_not_expired() {
        // 방금 쓴 캐시는 만료되지 않아야 함.
        let dir = std::env::temp_dir().join(format!("vworld-test-exp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".update-check");

        write_cache(&path, "v1.0.0");
        let (last_check, _) = read_cache(&path).unwrap();
        let age = now_epoch().saturating_sub(last_check);
        assert!(age < CHECK_INTERVAL_SECS, "방금 쓴 캐시가 만료됨 — age={age}s");

        std::fs::remove_dir_all(&dir).ok();
    }
}
