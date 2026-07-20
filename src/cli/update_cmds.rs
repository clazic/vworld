//! `vworld update` 서브커맨드 — 바이너리 자가교체 + 스킬 파일 갱신.

use crate::update;
use anyhow::{anyhow, Context, Result};
use clap::Args;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

/// `vworld update` 인자.
///
/// GitHub Releases에서 최신(또는 지정한) 버전을 내려받아 현재 실행 중인 바이너리를 교체하고,
/// 이어서 설치된 스킬 디렉터리(`~/.claude/skills/vworld`, `~/.codex/skills/vworld` 등)의
/// 문서·레퍼런스 파일도 함께 갱신한다. 각 단계는 개별 확인 프롬프트를 거치며,
/// 다운로드한 자산은 릴리스의 `SHA256SUMS`로 검증한다. 인증키가 필요 없는 오프라인 명령이다.
///
/// 참고: 평소 실행 시 하루 1회 자동으로 새 버전을 감지·알림(stderr, 다운로드는 하지 않음).
/// 이 자동 알림을 끄려면 환경변수 `VWORLD_NO_UPDATE_CHECK=1`을 설정한다(CI 환경은 자동 생략).
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// 실제 교체 없이 최신 버전 확인만.
    ///
    /// 현재 버전과 GitHub의 최신 태그를 비교해 새 버전 유무만 알린다.
    /// 다운로드·교체는 수행하지 않는다.
    #[arg(long)]
    pub check: bool,

    /// 특정 버전 태그로 교체 (예: v0.2.0). 미지정 시 최신 버전.
    ///
    /// 최신 버전이 아닌 특정 릴리스로 고정하고 싶을 때(회귀 발생 시 롤백 등) 사용한다.
    /// 값은 GitHub Releases 태그명과 동일한 형식(`v` 접두 포함)이어야 한다.
    /// 버전 비교를 건너뛰므로 현재보다 낮은 버전으로도 내려갈 수 있다.
    #[arg(long, value_name = "TAG")]
    pub version: Option<String>,

    /// 확인 없이 즉시 교체(바이너리·스킬 프롬프트 모두 y로 간주).
    ///
    /// CI·스크립트 등 비대화형 환경에서 지정한다. 지정하지 않으면 stdin이 터미널이
    /// 아닐 때 프롬프트는 자동으로 "아니오"로 처리되어 아무것도 바꾸지 않는다.
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// 같은 버전이어도 다시 내려받아 교체.
    ///
    /// 설치본이 손상됐거나 스킬 파일만 복구하고 싶을 때 사용한다.
    #[arg(long)]
    pub force: bool,

    /// 바이너리는 그대로 두고 스킬 파일만 갱신.
    #[arg(long = "skill-only", conflicts_with = "no_skill")]
    pub skill_only: bool,

    /// 스킬 파일은 건너뛰고 바이너리만 교체.
    #[arg(long = "no-skill")]
    pub no_skill: bool,
}

/// y/N 프롬프트. `--yes`면 무조건 true, 비대화형(파이프·CI)이면 false.
fn prompt_yn(msg: &str, assume_yes: bool) -> bool {
    if assume_yes {
        eprintln!("{msg}y");
        return true;
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("{msg}N (비대화형 — 건너뜀)");
        return false;
    }
    eprint!("{msg}");
    let _ = std::io::stderr().flush();

    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(n) if n > 0 => line.trim().eq_ignore_ascii_case("y"),
        _ => false, // EOF → 아니오
    }
}

pub async fn run_update(args: UpdateArgs) -> Result<()> {
    // 이 명령 자체가 버전을 다루므로 종료 직전 자동 알림은 끈다.
    update::suppress_notify();
    let current = env!("CARGO_PKG_VERSION");
    eprintln!("현재 버전: v{current}");

    // 대상 태그 결정 — --version 지정 시 비교 없이 그 태그로.
    let tag = match &args.version {
        Some(t) => {
            let t = if t.starts_with('v') { t.clone() } else { format!("v{t}") };
            eprintln!("대상 버전: {t}");
            t
        }
        None => {
            eprintln!("최신 버전 확인 중...");
            let latest = update::fetch_latest_tag().await?;
            eprintln!("최신 버전: {latest}");

            if args.check {
                if update::is_newer(&latest, current) {
                    eprintln!("`vworld update` 를 실행하여 업데이트하세요.");
                } else {
                    eprintln!("이미 최신 버전입니다.");
                }
                return Ok(());
            }
            if !update::is_newer(&latest, current) && !args.force {
                eprintln!("이미 최신 버전입니다.");
                return Ok(());
            }
            latest
        }
    };

    if args.check {
        return Ok(());
    }

    // 체크섬 파일 — 구 릴리스에는 없을 수 있으므로 없으면 경고 후 진행.
    eprintln!("  체크섬 파일 다운로드 중...");
    let sums = match update::download_asset(&tag, update::CHECKSUM_ASSET).await? {
        Some(bytes) => update::parse_sha256sums(&String::from_utf8_lossy(&bytes)),
        None => {
            eprintln!(
                "  경고: 이 릴리스에는 {} 가 없습니다. 체크섬 검증을 생략합니다.",
                update::CHECKSUM_ASSET
            );
            Default::default()
        }
    };

    let mut binary_updated = false;
    let mut new_binary: Option<Vec<u8>> = None;

    // ── 바이너리 ────────────────────────────────────────────────────────────
    if !args.skill_only {
        let asset = update::binary_asset_name()?;
        eprintln!("  바이너리 다운로드 중 ({asset})...");
        let bytes = update::download_asset(&tag, asset)
            .await?
            .ok_or_else(|| anyhow!("릴리스 {tag} 에 자산 {asset} 이 없습니다"))?;

        if prompt_yn(
            &format!("바이너리를 업데이트하겠습니까? (v{current} → {tag}) (y/N) "),
            args.yes,
        ) {
            verify(&sums, asset, &bytes, "SHA256 체크섬 검증 중...", "체크섬 검증 완료.")?;

            let exe = std::env::current_exe().context("현재 실행 파일 경로 확인 실패")?;
            // 심링크로 설치된 경우 실제 파일을 교체해야 한다.
            let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
            eprintln!("  바이너리 교체 중 ({})...", exe.display());
            update::replace_binary(&bytes, &exe)?;

            // macOS Gatekeeper quarantine 속성 제거 (best-effort).
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("xattr")
                    .args(["-dr", "com.apple.quarantine"])
                    .arg(&exe)
                    .status();
            }

            eprintln!("  바이너리: {}", exe.display());
            binary_updated = true;
            new_binary = Some(bytes);
        }
    }

    // ── 스킬 파일 ───────────────────────────────────────────────────────────
    let mut skill_updated = false;
    if !args.no_skill {
        let skill_dirs = update::collect_skill_dirs();
        if skill_dirs.is_empty() {
            eprintln!("  스킬 디렉터리가 없어 스킬 갱신을 건너뜁니다.");
        } else if prompt_yn(
            "스킬 파일(SKILL.md, INSTALL.md, references 등)을 업데이트하겠습니까? (y/N) ",
            args.yes,
        ) {
            eprintln!("  스킬 파일 다운로드 중...");
            match update::download_asset(&tag, update::SKILL_ASSET).await? {
                Some(bytes) => {
                    verify(
                        &sums,
                        update::SKILL_ASSET,
                        &bytes,
                        "스킬 체크섬 검증 중...",
                        "스킬 체크섬 검증 완료.",
                    )?;
                    for dir in &skill_dirs {
                        update::extract_zip(&bytes, dir)
                            .with_context(|| format!("스킬 갱신 실패: {}", dir.display()))?;
                        refresh_skill_binary(dir, new_binary.as_deref());
                        eprintln!("  스킬: {}", dir.display());
                    }
                    skill_updated = true;
                }
                None => eprintln!(
                    "  경고: 릴리스 {tag} 에 {} 가 없어 스킬 갱신을 건너뜁니다.",
                    update::SKILL_ASSET
                ),
            }
        }
    }

    if binary_updated || skill_updated {
        eprintln!("vworld v{current} → {tag} 업데이트 완료");
    } else {
        eprintln!("변경된 항목이 없습니다.");
    }
    Ok(())
}

/// 체크섬 맵에 항목이 있으면 검증, 없으면 경고만.
fn verify(
    sums: &std::collections::HashMap<String, String>,
    asset: &str,
    bytes: &[u8],
    doing: &str,
    done: &str,
) -> Result<()> {
    match sums.get(asset) {
        Some(expected) => {
            eprintln!("  {doing}");
            update::verify_sha256(bytes, expected)?;
            eprintln!("  {done}");
        }
        None => eprintln!("  경고: {asset} 의 체크섬 항목이 없어 검증을 생략합니다."),
    }
    Ok(())
}

/// 스킬 디렉터리에 `app/vworld` 사본이 있으면 새 바이너리로 함께 갱신한다.
/// install.sh 가 배치한 사본이 stale로 남는 문제를 막는다(없으면 아무것도 하지 않음).
fn refresh_skill_binary(skill_dir: &PathBuf, new_binary: Option<&[u8]>) {
    let Some(bytes) = new_binary else { return };
    let name = if cfg!(windows) { "vworld.exe" } else { "vworld" };
    let path = skill_dir.join("app").join(name);
    if !path.is_file() {
        return;
    }
    match update::replace_binary(bytes, &path) {
        Ok(()) => eprintln!("  스킬 바이너리: {}", path.display()),
        Err(e) => eprintln!("  경고: 스킬 바이너리 갱신 실패 ({e})"),
    }
}
