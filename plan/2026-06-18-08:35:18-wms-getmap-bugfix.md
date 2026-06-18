# WMS GetMap 버그 수정 계획

작성: 2026-06-18 / 담당: Sonnet(구현), 분석: Opus
대상: `src/cli/data_cmds.rs` (run_wms, WmsArgs), `src/api/mod.rs`

## 배경
두 WMS 가이드(`v4dv_wmsguide`=구 `map.vworld.kr/js/wms.do`, `v4dv_wmsguide2`=현 `api.vworld.kr/req/wms`)는 **동일 백엔드의 신·구 문서**(GetMap 응답 byte 동일). guide 추가 구현은 실익 없음. 대신 현재 `wms` 명령의 GetMap이 깨져 있어 수정한다.

## 🐞 버그 리스트

| # | 버그 | 원인 | 증상 | 심각도 |
|---|------|------|------|--------|
| **B1** | GetMap이 `version=2.0` 강제 | `QueryBuilder::new`가 모든 요청에 `version=2.0` 주입(`api/mod.rs:43`), `run_wms`에 `.version()` 교체 없음 | 서버가 `INVALID_RANGE` ServiceException 반환(유효값 `[1.3.0]`). **GetMap 전면 실패** | 치명 |
| **B2** | 이미지를 JSON으로 파싱 | `fetch_one`이 `get_text`+`parse_to_json`(`data_cmds.rs:42-46`)으로 처리 | version을 고쳐도 PNG를 JSON 파싱하다 실패. **이미지 저장 경로 부재** | 치명 |
| **B3** | GetMap 핵심 파라미터 누락 | `WmsArgs`에 `styles`/`format`/`transparent` 없음(`data_cmds.rs:1095-1109`) | 레이어 스타일·투명배경·포맷 제어 불가. OGC 정석 호출 미흡 | 중 |
| **B4** | 출력 플래그 부재 | `WmsArgs`에 `--output` 없음 | GetMap 결과(이미지)를 파일로 저장할 방법 없음 | 중 |
| B5 | BBOX 축순서 미안내 | EPSG:4326·5185~5188은 `(위도,경도)` 순(1.3.0) | 사용자가 `(경도,위도)`로 넣으면 빈/오위치. (자동 swap은 위험 → 문서 안내만) | 낮 |

> GetCapabilities는 우연히 동작(서버가 version=2.0 무시하고 1.3.0 반환)해 버그가 가려져 있었음.

## ✅ 수정 계획

### 수정 1 — version 교정 (B1)
`run_wms`에서 `QueryBuilder::new("wms", &a.request).version("1.3.0")`. (WFS의 `.version("1.1.0")` 전례와 동일)

### 수정 2 — GetMap 이미지 출력 경로 (B2, B4)
- `WmsArgs`에 `--output/-o: Option<PathBuf>` 추가.
- `run_wms`: request가 `GetMap`(대소문자 무시)이면 `client.get_bytes()`(이미 존재, `api/mod.rs:276` + `guard_image_error`)로 받아 `output::save_bytes()`로 저장.
  - GetMap인데 `--output` 미지정이면 명확한 에러("GetMap은 -o <file.png> 필요").
- GetCapabilities/GetFeatureInfo 등 텍스트 응답은 기존 `fetch_one` 경로 유지.

### 수정 3 — GetMap 파라미터 확충 (B3)
`WmsArgs`에 추가:
- `styles: Option<String>` (레이어별 스타일, 생략 시 기본)
- `format: String` (기본 `image/png`)
- `transparent: bool` (투명 배경 flag → `TRUE`/`FALSE`)
GetMap 요청에만 format/transparent 주입.

### 수정 4 — 문서 안내 (B5)
help/USAGE에 "EPSG:4326은 bbox가 `(ymin,xmin,ymax,xmax)`=위도,경도 순" 명시. 자동 변환은 하지 않음(위험).

## 검증 계획
1. `cargo build` 통과
2. GetCapabilities — 기존처럼 XML 반환(회귀 없음)
3. GetMap — `--layers <레이어> --bbox <4326 위경도> --width 512 --height 512 -o out.png` → 실제 PNG 저장(파일 크기 > 0, PNG 시그니처)
4. GetMap `--transparent` → 투명 PNG
5. 회귀 테스트 30/30
6. 4-OS 재빌드 + skills/app 갱신 + 배포 zip 갱신

## 비고
- `guide` 추가 구현은 **하지 않음**(동일 백엔드, 구 엔드포인트라 기능 열위).
- `staticmap`(`/req/image`)과의 차이: WMS GetMap은 임의 레이어 조합·투명배경·스타일 지정이 가능(staticmap은 고정 배경도).
