//! 통합 테스트 — lib로 노출된 실제 코드 경로를 검증(외부 API 호출 없음).

use std::time::Duration;
use vworld::api::Auth;

/// 테스트별 고유 임시 경로 — 병렬 테스트·동시 CI 충돌 방지.
fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("vworld_it_{}_{name}", std::process::id()))
}

#[test]
fn save_bytes_writes_file() {
    let path = temp_path("save.bin");
    let saved = vworld::output::save_bytes(&path, b"test data").unwrap();
    assert_eq!(saved.path, path);
    assert_eq!(saved.bytes, 9);
    assert_eq!(std::fs::read(&path).unwrap(), b"test data");
    std::fs::remove_file(&path).ok();
}

#[test]
fn batch_item_serialization() {
    let ok = vworld::output::BatchItem { index: 0, result: Some(serde_json::json!({"value": 42})), error: None };
    let json = serde_json::to_string(&ok).unwrap();
    assert!(json.contains("\"index\":0") && json.contains("\"value\":42") && !json.contains("\"error\""));

    let err = vworld::output::BatchItem { index: 1, result: None, error: Some(serde_json::json!({"message": "failed"})) };
    let json = serde_json::to_string(&err).unwrap();
    assert!(json.contains("\"index\":1") && !json.contains("\"result\"") && json.contains("\"message\":\"failed\""));
}

#[test]
fn wfs_viewer_blocks_script_breakout_and_keeps_object() {
    let geojson = r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":"</script><script>alert(1)</script>"},"geometry":{"type":"Point","coordinates":[127.0,37.5]}}]}"#;
    let html = vworld::cli::embed_cmds::render_wfs_viewer(geojson, None, "KEY", "");
    // 페이로드가 </script>로 스크립트 블록을 닫지 못해야 함.
    assert!(!html.contains("</script><script>alert(1)"));
    // GeoJSON은 문자열이 아니라 객체 리터럴로 주입돼야 L.geoJSON이 동작.
    assert!(html.contains(r#"var data = {"type":"FeatureCollection""#));
}

#[tokio::test]
async fn run_jobs_preserves_input_order() {
    // 앞 작업일수록 늦게 끝나도록 지연 → 완료 순서는 역순, 결과는 입력 순서여야 함.
    let jobs: Vec<u64> = (0..5).collect();
    let keys = vec![Auth { key: "k".into(), domain: None }];
    let out = vworld::concurrency::run_jobs(jobs, keys, 5, |j, _auth| async move {
        tokio::time::sleep(Duration::from_millis((5 - j) * 20)).await;
        Ok(j)
    })
    .await;
    let got: Vec<u64> = out.into_iter().map(|r| r.unwrap()).collect();
    assert_eq!(got, vec![0, 1, 2, 3, 4]);
}
