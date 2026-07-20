//! 자가 업데이트 — GitHub Releases 기반 self-update 및 주기적 버전 감지.
//!
//! 파이프라인(sgis 동형): 최신 태그 조회 → SHA256SUMS → 바이너리 다운로드 → 확인 프롬프트
//! → 체크섬 검증 → 바이너리 교체 → 스킬 파일 갱신.
//!
//! - `fetch_latest_tag()`: 최신 릴리즈 태그 조회
//! - `download_asset()`: 릴리스 자산 다운로드(404 구분)
//! - `replace_binary()`: 실행 중인 바이너리 교체(Windows `.old` 선이동)
//! - `collect_skill_dirs()`: 설치된 스킬 디렉터리 탐지(존재하는 것만)
//! - `maybe_notify()`: 하루 1회 체크 후 새 버전이 있으면 stderr 알림(자동 교체 안 함)

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REPO_OWNER: &str = "clazic";
const REPO_NAME: &str = "vworld";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// 업데이트 체크 캐시 만료 시간 (24시간).
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
/// 릴리스에 함께 게시되는 체크섬 파일명.
pub const CHECKSUM_ASSET: &str = "SHA256SUMS";
/// 스킬 파일 전용 자산(바이너리 미포함).
pub const SKILL_ASSET: &str = "vworld-skill-files.zip";
/// 스킬 디렉터리 이름.
const SKILL_NAME: &str = "vworld";

/// `update` 명령 실행 시 종료 직전 버전 알림을 끄기 위한 플래그.
static NOTIFY_SUPPRESSED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 이번 프로세스에서는 종료 직전 버전 알림을 출력하지 않는다.
pub fn suppress_notify() {
    NOTIFY_SUPPRESSED.store(true, std::sync::atomic::Ordering::Relaxed);
}

// ─────────────────────────── OS 타깃 매핑 ────────────────────────────────────

/// 현재 플랫폼에 맞는 릴리스 자산명.
/// release.yml 자산: vworld-macos(universal) / vworld-linux(x86_64) / vworld-windows.exe.
pub fn binary_asset_name() -> Result<&'static str> {
    #[cfg(target_os = "macos")]
    return Ok("vworld-macos");

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Ok("vworld-linux");

    #[cfg(target_os = "windows")]
    return Ok("vworld-windows.exe");

    #[cfg(all(target_os = "linux", not(target_arch = "x86_64")))]
    return Err(anyhow!(
        "이 플랫폼(linux/{})용 릴리스 자산이 없습니다. 소스에서 빌드하세요: cargo install --path .",
        std::env::consts::ARCH
    ));

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return Err(anyhow!(
        "지원하지 않는 플랫폼입니다. macOS / Linux / Windows만 지원합니다."
    ));
}

// ─────────────────────────── 버전 비교 ───────────────────────────────────────

/// "v1.2.3" → "1.2.3" 정규화.
pub fn strip_v(version: &str) -> &str {
    version.trim_start_matches('v')
}

/// 점 구분 세그먼트를 숫자로 파싱(비숫자 세그먼트는 0).
fn version_parts(version: &str) -> Vec<u64> {
    strip_v(version)
        .split(['.', '-', '+'])
        .map(|seg| {
            let digits: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

/// `latest` > `current`이면 true.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let l = version_parts(latest);
    let c = version_parts(current);
    let n = l.len().max(c.len());
    for i in 0..n {
        let a = l.get(i).copied().unwrap_or(0);
        let b = c.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

// ─────────────────────────── HTTP ────────────────────────────────────────────

/// GitHub API/다운로드용 클라이언트. User-Agent 없으면 GitHub API가 403을 준다.
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("vworld/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(300))
        .build()
        .context("HTTP 클라이언트 생성 실패")
}

/// GitHub Releases에서 최신 버전 태그를 가져온다.
pub async fn fetch_latest_tag() -> Result<String> {
    let url =
        format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");
    let resp = client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("최신 릴리즈 조회 실패")?;

    if !resp.status().is_success() {
        return Err(anyhow!("최신 릴리즈 조회 실패 (HTTP {})", resp.status()));
    }

    // reqwest 의 json feature 를 켜지 않았으므로 텍스트로 받아 파싱한다.
    let text = resp.text().await.context("릴리즈 응답 수신 실패")?;
    let body: serde_json::Value =
        serde_json::from_str(&text).context("릴리즈 응답 파싱 실패")?;
    body.get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("릴리즈 응답에 tag_name이 없습니다"))
}

/// 릴리스 자산 다운로드. 자산이 없으면(404) `Ok(None)`.
pub async fn download_asset(tag: &str, asset: &str) -> Result<Option<Vec<u8>>> {
    let url = format!(
        "https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/download/{tag}/{asset}"
    );
    let resp = client()?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("{asset} 다운로드 실패"))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(anyhow!("{asset} 다운로드 실패 (HTTP {})", resp.status()));
    }

    let bytes = resp
        .bytes()
        .await
        .with_context(|| format!("{asset} 본문 수신 실패"))?;
    Ok(Some(bytes.to_vec()))
}

// ─────────────────────────── 체크섬 ──────────────────────────────────────────

/// `SHA256SUMS` 본문 → {파일명: 해시} 맵. `*파일명`(바이너리 모드) 접두는 제거.
pub fn parse_sha256sums(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(hash), Some(name)) = (it.next(), it.next()) else {
            continue;
        };
        let name = name.trim_start_matches('*');
        // 경로가 섞여 있어도 파일명만으로 조회할 수 있게 basename으로 저장.
        let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        map.insert(name.to_string(), hash.to_ascii_lowercase());
    }
    map
}

/// 바이트열의 SHA256 16진 문자열.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 기대 해시와 대조. 불일치면 에러.
pub fn verify_sha256(bytes: &[u8], expected: &str) -> Result<()> {
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(anyhow!(
            "체크섬 불일치 — 기대 {expected}, 실제 {actual}. 다운로드가 손상되었거나 자산이 변조되었습니다."
        ))
    }
}

// ─────────────────────────── 바이너리 교체 ───────────────────────────────────

/// 새 바이너리로 `dst`를 교체한다.
///
/// 임시 파일은 반드시 `dst`와 **같은 디렉터리**에 만든다(다른 파일시스템이면 rename이 EXDEV로 실패).
/// Windows는 실행 중인 exe를 지울 수 없으므로 `.old`로 선이동한 뒤 교체한다.
pub fn replace_binary(new_bytes: &[u8], dst: &Path) -> Result<()> {
    let tmp = dst.with_extension("new");
    std::fs::write(&tmp, new_bytes)
        .with_context(|| format!("임시 파일 쓰기 실패: {}", tmp.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .context("실행 권한 설정 실패")?;
    }

    #[cfg(windows)]
    {
        let old = dst.with_extension("old");
        let _ = std::fs::remove_file(&old);
        if dst.exists() {
            std::fs::rename(dst, &old)
                .with_context(|| format!("기존 바이너리 이동 실패: {}", dst.display()))?;
        }
    }

    if let Err(e) = std::fs::rename(&tmp, dst) {
        let _ = std::fs::remove_file(&tmp);
        return Err(anyhow!(
            "바이너리 교체 실패: {} ({e}). 권한이 부족하면 sudo 또는 install.sh 재실행이 필요합니다.",
            dst.display()
        ));
    }
    Ok(())
}

// ─────────────────────────── 스킬 디렉터리 ───────────────────────────────────

/// 홈 디렉터리(Windows는 USERPROFILE).
pub fn home_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");

    home.map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| anyhow!("홈 디렉토리를 확인할 수 없습니다"))
}

/// 갱신 대상 스킬 디렉터리 — 후보 중 **이미 존재하는 것만** 반환.
/// 하나도 없으면 빈 벡터(설치되지 않은 환경이므로 아무것도 만들지 않는다).
pub fn collect_skill_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push_if_dir = |p: PathBuf| {
        if p.is_dir() && !dirs.contains(&p) {
            dirs.push(p);
        }
    };

    if let Ok(home) = home_dir() {
        push_if_dir(home.join(".claude").join("skills").join(SKILL_NAME));
        push_if_dir(home.join(".codex").join("skills").join(SKILL_NAME));
    }
    if let Ok(cwd) = std::env::current_dir() {
        push_if_dir(cwd.join(".claude").join("skills").join(SKILL_NAME));
        push_if_dir(cwd.join(".codex").join("skills").join(SKILL_NAME));
    }
    dirs
}

/// zip 바이트열을 `dest`에 해제. zip-slip은 `enclosed_name()`으로 차단.
pub fn extract_zip(bytes: &[u8], dest: &Path) -> Result<usize> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).context("스킬 zip 열기 실패")?;
    let mut written = 0usize;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("zip 항목 읽기 실패")?;
        // enclosed_name()은 `..`·절대경로를 거부한다(zip-slip 방어).
        let Some(rel) = entry.enclosed_name() else {
            return Err(anyhow!(
                "안전하지 않은 zip 경로가 포함되어 있습니다: {}",
                entry.name()
            ));
        };
        let out = dest.join(rel);

        if entry.is_dir() {
            std::fs::create_dir_all(&out)
                .with_context(|| format!("디렉터리 생성 실패: {}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("디렉터리 생성 실패: {}", parent.display()))?;
        }
        let mut file = std::fs::File::create(&out)
            .with_context(|| format!("파일 쓰기 실패: {}", out.display()))?;
        std::io::copy(&mut entry, &mut file)
            .with_context(|| format!("파일 쓰기 실패: {}", out.display()))?;
        written += 1;
    }
    Ok(written)
}

// ─────────────────────────── 캐시 경로 ───────────────────────────────────────

/// 업데이트 체크 캐시 파일 경로: `~/.vworld/.update-check`.
pub fn cache_path() -> Result<PathBuf> {
    Ok(home_dir()?.join(".vworld").join(".update-check"))
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
pub async fn maybe_notify() {
    // CI 환경 또는 사용자가 명시적으로 끈 경우 생략.
    if std::env::var_os("CI").is_some() || std::env::var_os("VWORLD_NO_UPDATE_CHECK").is_some() {
        return;
    }
    // `vworld update` 직후에는 알리지 않는다 — 실행 중인 프로세스는 교체 전 버전이라
    // 방금 설치한 새 버전을 "새 버전이 있습니다"로 다시 안내하게 된다.
    if NOTIFY_SUPPRESSED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }

    let Ok(cache) = cache_path() else { return };

    // 24시간 게이트: 아직 만료 안 됐으면 캐시된 태그로만 비교.
    let now = now_epoch();
    if let Some((last_check, latest_tag)) = read_cache(&cache) {
        if now.saturating_sub(last_check) < CHECK_INTERVAL_SECS {
            if is_newer(&latest_tag, CURRENT_VERSION) {
                eprintln!(
                    "\n[vworld] 새 버전 {latest_tag} 이 있습니다. \
                    `vworld update` 를 실행하여 업데이트하세요."
                );
            }
            return;
        }
    }

    // 캐시 만료 → GitHub API 조회. 네트워크 실패는 조용히 무시.
    let Ok(latest_tag) = fetch_latest_tag().await else {
        return;
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
        assert!(is_newer("v0.10.0", "0.9.0")); // 문자열 비교였다면 실패할 케이스
        assert!(is_newer("v1.0.0", "0.99.99"));
        assert!(!is_newer("v0.2.1", "0.2.1"));
    }

    #[test]
    fn test_binary_asset_name() {
        // 지원 플랫폼에서는 릴리스 자산명이 나와야 한다.
        let name = binary_asset_name().expect("지원 플랫폼이어야 함");
        assert!(
            ["vworld-macos", "vworld-linux", "vworld-windows.exe"].contains(&name),
            "예상치 못한 자산명: {name}"
        );
    }

    #[test]
    fn test_parse_sha256sums() {
        let text = "abc123  vworld-macos\ndef456 *vworld-skill-files.zip\n\
                    99ff  ./artifacts/linux/vworld-linux\n잘못된줄\n";
        let map = parse_sha256sums(text);
        assert_eq!(map.get("vworld-macos").unwrap(), "abc123");
        // '*'(바이너리 모드) 접두 제거.
        assert_eq!(map.get("vworld-skill-files.zip").unwrap(), "def456");
        // 경로가 섞여 있어도 basename으로 조회 가능.
        assert_eq!(map.get("vworld-linux").unwrap(), "99ff");
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn test_verify_sha256() {
        // 빈 입력의 SHA256은 널리 알려진 상수.
        let empty = sha256_hex(b"");
        assert_eq!(
            empty,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(verify_sha256(b"", &empty.to_uppercase()).is_ok()); // 대소문자 무시
        assert!(verify_sha256(b"tampered", &empty).is_err());
    }

    #[test]
    fn test_extract_zip_rejects_traversal() {
        // `..`를 포함한 경로는 거부되어야 한다(zip-slip).
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            w.start_file("../evil.txt", opts).unwrap();
            std::io::Write::write_all(&mut w, b"pwned").unwrap();
            w.finish().unwrap();
        }
        let dir = std::env::temp_dir().join(format!("vworld-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let result = extract_zip(&buf, &dir);
        assert!(result.is_err(), "zip-slip 경로가 통과됨");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_extract_zip_writes_files() {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            w.start_file("SKILL.md", opts).unwrap();
            std::io::Write::write_all(&mut w, b"# skill").unwrap();
            w.start_file("references/docs/USAGE.md", opts).unwrap();
            std::io::Write::write_all(&mut w, b"usage").unwrap();
            w.finish().unwrap();
        }
        let dir = std::env::temp_dir().join(format!("vworld-zip-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let n = extract_zip(&buf, &dir).unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read_to_string(dir.join("SKILL.md")).unwrap(), "# skill");
        assert!(dir.join("references/docs/USAGE.md").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_replace_binary() {
        let dir = std::env::temp_dir().join(format!("vworld-bin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dst = dir.join("vworld");
        std::fs::write(&dst, b"old").unwrap();

        replace_binary(b"new", &dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"new");
        // 임시 파일은 남지 않아야 한다.
        assert!(!dst.with_extension("new").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cache_rw() {
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
