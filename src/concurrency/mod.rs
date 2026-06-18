//! 동시성 — 키 풀(반납형) + 작업 큐 + 워커 + 재시도(kosis 패턴의 Rust 매핑, 설계 §3).

pub mod key_pool;
pub mod retry;
pub mod worker;

pub use retry::FailKind;
pub use worker::run_jobs;
