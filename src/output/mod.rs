//! JSON 직렬화·출력 헬퍼 — `--pretty`/`--raw`, 파일 저장, 에러의 JSON 친화 출력(설계 §5).

use crate::cli::GlobalArgs;
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 데이터형 결과를 stdout에 JSON으로 출력. `--pretty`면 들여쓰기.
pub fn print_json<T: Serialize>(g: &GlobalArgs, value: &T) -> anyhow::Result<()> {
    let s = if g.pretty {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };
    let mut out = std::io::stdout().lock();
    out.write_all(s.as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}

/// 이미 직렬화된 원문 텍스트(`--raw` 데이터형/렌더링형)를 그대로 출력.
pub fn print_raw_text(text: &str) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}

/// 바이트를 stdout으로(이미지형 `--raw`, 파이프용).
pub fn print_raw_bytes(bytes: &[u8]) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(bytes)?;
    Ok(())
}

/// 바이트를 파일로 저장하고 경로·바이트수 보고(이미지형 기본).
pub fn save_bytes(path: &Path, bytes: &[u8]) -> anyhow::Result<SavedFile> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, bytes)?;
    Ok(SavedFile {
        path: path.to_path_buf(),
        bytes: bytes.len(),
    })
}

/// 파일 저장 결과(JSON 보고용).
#[derive(Debug, Serialize)]
pub struct SavedFile {
    pub path: PathBuf,
    pub bytes: usize,
}

/// 에러를 JSON 친화 형식으로 stderr에 출력.
pub fn print_error(err: &anyhow::Error) {
    let payload = serde_json::json!({
        "ok": false,
        "error": {
            "message": err.to_string(),
            "chain": err.chain().skip(1).map(|c| c.to_string()).collect::<Vec<_>>(),
        }
    });
    let s = serde_json::to_string(&payload).unwrap_or_else(|_| err.to_string());
    eprintln!("{s}");
}

/// 배치 결과 1건 — 입력 순서(`index`) 보존(설계 §5.1).
#[derive(Debug, Serialize)]
pub struct BatchItem {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}
