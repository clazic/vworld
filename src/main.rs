//! VWorld OpenAPI CLI — 자기완결 단일 바이너리.
//!
//! 설계: `.omc/plans/2026-06-16-vworld-cli-ai-구현계획.md`
//! 레퍼런스: `references/docs/rest_api_catalog.md`, `references/docs/national_data_catalog.md`

mod api;
mod cli;
mod concurrency;
mod config;
mod dxf;
mod geomath;
mod hjd_db;
mod hjd_shp;
mod ned_registry;
mod output;
mod shp;
mod twod_registry;
mod update;

use clap::Parser;
use cli::Cli;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    // tokio 멀티스레드 런타임 구성 (워커 = --concurrency, §3 동시성 모델).
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("런타임 초기화 실패: {e}");
            return ExitCode::FAILURE;
        }
    };

    let exit = match runtime.block_on(cli::run(cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // 에러는 JSON 친화 형식으로 stderr 출력 (§5 출력 규약).
            output::print_error(&e);
            ExitCode::FAILURE
        }
    };

    // 평소 실행 시 하루 1회 버전 감지 — best-effort, 종료코드와 독립.
    runtime.block_on(update::maybe_notify());

    exit
}
