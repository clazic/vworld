//! `vworld update` 서브커맨드 — 명시적 자가교체.

use anyhow::Result;
use clap::Args;

/// `vworld update` 인자.
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// 실제 교체 없이 최신 버전 확인만.
    #[arg(long)]
    pub check: bool,

    /// 특정 버전 태그로 교체 (예: v0.2.0). 미지정 시 최신 버전.
    #[arg(long, value_name = "TAG")]
    pub version: Option<String>,

    /// 확인 없이 즉시 교체.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub async fn run_update(args: UpdateArgs) -> Result<()> {
    let target = crate::update::os_target()?;
    let current = env!("CARGO_PKG_VERSION");

    if args.check {
        // 최신 버전 조회만, 교체 없음.
        let latest = tokio::task::spawn_blocking(crate::update::fetch_latest_tag)
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking 실패: {e}"))??;

        if crate::update::is_newer(&latest, current) {
            eprintln!("새 버전 {latest} 이 있습니다. 현재: v{current}");
            eprintln!("`vworld update` 를 실행하여 업데이트하세요.");
        } else {
            eprintln!("이미 최신 버전입니다 (v{current}).");
        }
        return Ok(());
    }

    // 실제 교체.
    let version_tag = args.version.clone();
    let no_confirm = args.yes;

    eprintln!("vworld 업데이트를 시작합니다 (현재: v{current}, 대상: {target})...");

    let status = tokio::task::spawn_blocking(move || {
        crate::update::run_self_update(target, version_tag.as_deref(), no_confirm)
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking 실패: {e}"))??;

    match status {
        self_update::Status::UpToDate(v) => {
            eprintln!("이미 최신 버전입니다 ({v}).");
        }
        self_update::Status::Updated(v) => {
            eprintln!("✔ v{v} 로 업데이트 완료.");
            eprintln!("변경사항 적용을 위해 vworld 를 다시 실행하세요.");
            eprintln!(
                "참고: 스킬/다른 경로에 설치된 사본(app/vworld-macos 등, ~/.local/bin 등)은 \
                install.sh 를 다시 실행하여 갱신하세요."
            );

            // macOS Gatekeeper quarantine 속성 제거 (best-effort).
            #[cfg(target_os = "macos")]
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new("xattr")
                    .args(["-dr", "com.apple.quarantine"])
                    .arg(&exe)
                    .status();
            }
        }
    }

    Ok(())
}
