//! `vworld update` 서브커맨드 — 명시적 자가교체.

use anyhow::Result;
use clap::Args;

/// `vworld update` 인자.
///
/// GitHub Releases에서 최신(또는 지정한) 버전을 내려받아 현재 실행 중인 바이너리를 교체한다.
/// 키·설정 파일과 무관한 오프라인 명령이며 인증키가 필요 없다. 스킬 모드로 여러 사본이
/// 설치된 경우 이 명령은 실행 중인 바이너리 1개만 교체하므로, 다른 경로의 사본은
/// install.sh(또는 설치 스크립트)를 다시 실행해 갱신해야 한다.
///
/// 참고: 평소 실행 시 하루 1회 자동으로 새 버전을 감지·알림(stderr, 다운로드는 하지 않음).
/// 이 자동 알림을 끄려면 환경변수 `VWORLD_NO_UPDATE_CHECK=1`을 설정한다(CI 환경은 자동 생략).
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// 실제 교체 없이 최신 버전 확인만.
    ///
    /// 현재 버전과 GitHub의 최신 태그를 비교해 새 버전 유무만 stderr로 알린다.
    /// 다운로드·교체는 수행하지 않는다. `vworld update --check`로 먼저 확인 후
    /// 필요하면 `--check` 없이 다시 실행해 실제 교체한다.
    #[arg(long)]
    pub check: bool,

    /// 특정 버전 태그로 교체 (예: v0.2.0). 미지정 시 최신 버전.
    ///
    /// 최신 버전이 아닌 특정 릴리스로 고정하고 싶을 때(회귀 발생 시 롤백 등) 사용한다.
    /// 값은 GitHub Releases 태그명과 동일한 형식(`v` 접두 포함)이어야 한다.
    #[arg(long, value_name = "TAG")]
    pub version: Option<String>,

    /// 확인 없이 즉시 교체.
    ///
    /// 대화형 확인 프롬프트를 생략하고 바로 다운로드·교체를 진행한다.
    /// CI·스크립트 등 비대화형 환경에서 필수로 지정한다.
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
