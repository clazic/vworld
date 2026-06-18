//! 설정·키 관리 명령 핸들러(설계 §4, ⑥).

use super::GlobalArgs;
use crate::api::{Auth, Client, QueryBuilder};
use crate::api::normalize;
use crate::config::{keys, resolve_config_path, Config};
use crate::output;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// API 키 추가(중복 거부). 별칭·도메인 선택.
    AddKey {
        key: String,
        #[arg(long)]
        alias: Option<String>,
        /// 도메인 등록 키의 referer/domain.
        #[arg(long)]
        referer: Option<String>,
    },
    /// 등록 키 목록(마스킹).
    ListKeys,
    /// 키 제거(값 또는 인덱스).
    RemoveKey { target: String },
    /// 각 키를 실 VWorld 호출로 검증(유효/만료/도메인불일치).
    TestKeys,
    /// 설정파일 실제 경로 출력.
    Path,
}

pub async fn run(g: &GlobalArgs, cmd: ConfigCmd) -> Result<()> {
    match cmd {
        ConfigCmd::AddKey {
            key,
            alias,
            referer,
        } => add_key(g, key, alias, referer),
        ConfigCmd::ListKeys => list_keys(g),
        ConfigCmd::RemoveKey { target } => remove_key(g, &target),
        ConfigCmd::TestKeys => test_keys(g).await,
        ConfigCmd::Path => show_path(g),
    }
}

/// add-key 등 쓰기 명령은 경로가 없어도 기본 경로를 사용(파일 없으면 새로 생성).
fn writable_path(g: &GlobalArgs) -> Result<std::path::PathBuf> {
    match &g.config {
        Some(p) => Ok(p.clone()),
        None => crate::config::default_config_path(),
    }
}

fn add_key(g: &GlobalArgs, key: String, alias: Option<String>, referer: Option<String>) -> Result<()> {
    let path = writable_path(g)?;
    let mut cfg = if path.exists() {
        Config::load(&path)?
    } else {
        Config::default()
    };
    let masked = keys::mask(&key);
    keys::add_key(&mut cfg, key, alias, referer)?;
    cfg.save(&path)?;
    output::print_json(
        g,
        &serde_json::json!({"ok": true, "added": masked, "path": path}),
    )
}

fn list_keys(g: &GlobalArgs) -> Result<()> {
    let path = resolve_config_path(g.config.as_deref())?;
    let cfg = Config::load(&path)?;
    output::print_json(g, &serde_json::json!({"ok": true, "keys": keys::list_masked(&cfg)}))
}

fn remove_key(g: &GlobalArgs, target: &str) -> Result<()> {
    let path = resolve_config_path(g.config.as_deref())?;
    let mut cfg = Config::load(&path)?;
    let removed = keys::remove_key(&mut cfg, target)?;
    cfg.save(&path)?;
    output::print_json(
        g,
        &serde_json::json!({"ok": true, "removed": keys::mask(&removed.key), "path": path}),
    )
}

fn show_path(g: &GlobalArgs) -> Result<()> {
    // path는 존재하지 않아도 기본 경로를 보고(검증 목적).
    let path = match &g.config {
        Some(p) => p.clone(),
        None => crate::config::default_config_path()?,
    };
    output::print_json(g, &serde_json::json!({"ok": true, "path": path, "exists": path.exists()}))
}

/// 실 VWorld 호출로 각 키 검증 — 지오코딩 1건으로 유효/도메인불일치/오류 판정.
async fn test_keys(g: &GlobalArgs) -> Result<()> {
    let path = resolve_config_path(g.config.as_deref())?;
    let cfg = Config::load(&path)?;
    if cfg.keys.is_empty() {
        return output::print_json(
            g,
            &serde_json::json!({"ok": true, "results": [], "note": "등록된 키 없음"}),
        );
    }
    let client = Client::new()?;
    let mut results = Vec::new();
    for entry in &cfg.keys {
        let auth = Auth {
            key: entry.key.clone(),
            domain: g.referer.clone().or_else(|| cfg.referer_for(entry)),
        };
        let (url, params) = QueryBuilder::new("address", "GetCoord")
            .format("json")
            .set("type", "ROAD")
            .set("address", "서울특별시 중구 세종대로 110")
            .build();
        let verdict = match client.get_text(&url, params, &auth).await {
            Ok(body) => match normalize::parse_to_json(&body) {
                Ok(v) => classify(&v, auth.domain.is_some()),
                Err(e) => format!("응답 파싱 실패: {e}"),
            },
            Err(e) => format!("호출 실패: {e}"),
        };
        results.push(serde_json::json!({
            "key": keys::mask(&entry.key),
            "alias": entry.alias,
            "verdict": verdict,
        }));
    }
    output::print_json(g, &serde_json::json!({"ok": true, "results": results}))
}

/// 응답 상태 → 사람 친화 판정. 도메인불일치 시 해결 가이드(§4.1).
fn classify(v: &serde_json::Value, has_domain: bool) -> String {
    match normalize::check_body_error(v) {
        Ok(()) => "유효".into(),
        Err(e) if e.empty_ok => "유효(결과 없음)".into(),
        Err(e) => {
            let lower = format!("{} {}", e.code, e.text).to_lowercase();
            if lower.contains("domain") || lower.contains("도메인") || lower.contains("referer") {
                let guide = if has_domain {
                    "도메인 불일치 — config의 referer가 키 발급 도메인과 일치하는지 확인하거나 무도메인(서버) 키를 발급하세요."
                } else {
                    "도메인 불일치 — config에 referer를 설정하거나 --referer로 등록 도메인을 지정, 또는 무도메인(서버) 키를 발급하세요."
                };
                format!("도메인불일치 [{}]: {} | {}", e.code, e.text, guide)
            } else {
                format!("오류 [{}]: {}", e.code, e.text)
            }
        }
    }
}
