# vworld 3D 분석·시뮬레이션 15종 테스트 스토리

생성: `vworld map 3dsim --analysis <type> -o <file>.html`
검증: 로컬 HTTP 서버(`localhost:8765`) + 실제 vworld API 키 + Playwright(Chromium) 브라우저 자동화
스크린샷: `.vw_demo/shots/*.jpeg`

## 검증 등급
- ✅✅ **완전 동작 확인**: 분석/가시화를 실제로 트리거하여 결과(데이터·렌더)를 시각/데이터로 확인
- ✅ **로드 정상**: 지도·분석 모듈·UI 적재 정상, 콘솔 에러 0 (분석 트리거는 폴리곤 그리기 등 수동 인터랙션 필요)
- △ **부분 제약**: 지도는 정상이나 `modeler.js`(가상건물 편집) cross-origin 동적 import 제약 또는 클래스 미적재

---

## 1.0 분석·시뮬레이션 (11종)

### 1. slope — 경사도 분석 ✅✅
- **스토리**: "포인트 지정" 버튼 → 지도 클릭 → 클릭 지점 주변 지형의 경사도를 구간별로 계산.
- **검증 결과**: 분석 콜백이 실제 데이터 10구간 반환.
  | 경사각(°) | 경사도(%) | 색상 |
  |---|---|---|
  | 45~ | 100~ | #7c0014 |
  | 30~35 | 57.74~70.02 | #f50000 |
  | 25~30 | 46.63~57.74 | #ff7826 |
- 모듈: `tool3d/libapis/slope/slope_analysis_api.js`

### 2. terrainvolume — 토공량 분석 ✅
- **스토리**: 분석 영역(폴리곤)을 그린 뒤 고도·각도 입력 → 절토량/성토량/평균고도 계산.
- **검증**: `terrainVolume` 전역 객체 적재, 0 에러, `btnTerrainVolumeByPolygon` UI 표시.

### 3. profile — 지형 단면 분석 ✅
- **스토리**: 종단/횡단 라인을 그려 지형 단면(고도 프로파일)을 분석.
- **검증**: `profileTerrain` 적재, 0 에러.

### 4. sunlight — 일조량 분석 ✅
- **스토리**: 태양 위치 지점 지정 → 시간대별 일조량 분석(interval 1·5·10·15분).
- **검증**: `sunlightAnalysis` 적재, 0 에러.

### 5. sunlightrights — 일조권 분석 ✅
- **스토리**: 분석 지점 설정 → 일조권 분석 실행.
- **검증**: `sunlightrightsAnalysis` 적재, 0 에러.

### 6. sunlightslope — 일조권 사선제한 분석 ✅ (수정 완료)
- **스토리**: 필지 GeoJSON + 가상건물 → 일조권 사선제한 분석.
- **원인**: `modeler.js`가 cross-origin 스크립트라 `import()` base URL이 about:blank → 내부 ESM 5종(delaunator 등) 해석 실패.
- **해결**: modeler.js 스크립트 태그에 `crossorigin="anonymous"` 추가. map.vworld.kr이 `Access-Control-Allow-Origin: *`를 주므로 CORS-same-origin 스크립트로 취급되어 import base 정상화.
- **검증**: 콘솔 에러 0, `SunlightSlopeConstraint` 클래스 적재(function), `ws3de.modeler` 로드 확인.

### 7. visiblearea — 가시면적 분석 ✅ (수정 완료)
- **스토리**: 시점/종점 지정 → 가시/비가시 영역 분석.
- **원인**: `tool3d/page.js`가 `#subContainerToggle` 등 DOM 요소에 `addEventListener` 호출 → 요소 부재로 null 에러 → 스크립트 체인 중단, `VisibleArea` 미적재.
- **해결**: page.js가 참조하는 DOM stub 3개(`#viewerContainerWrapper`/`#subContainerToggle`/`#subContainer`)를 숨김으로 추가.
- **검증**: 콘솔 에러 0, `VisibleArea` 클래스 적재(function).

### 8. viewsurface — 시곡면 분석 ✅
- **스토리**: 분석 지점 선택(경사각/시곡각) → 시곡면 분석 + 차트.
- **검증**: 지도 정상, 0 에러.

### 9. culheritalter — 문화재 현상변경 분석 ✅ (수정 완료)
- **스토리**: 문화재 구역 + 가상건물 → 고도제한·앙각규정 분석.
- **원인**: (a) 샘플에 `modeler.js` 누락, (b) cross-origin import base 문제.
- **해결**: `culHeritAlter_api.js` 뒤에 `crossorigin="anonymous"` modeler.js 추가.
- **검증**: 콘솔 에러 0, `CulHeritAlter` 클래스 적재(function), `ws3de.modeler` 로드.

### 10. route — 드론·차량 모의주행 시뮬레이션 ✅✅
- **스토리**: GeoJSON 경로 생성 → 드론/차량 모의주행 재생.
- **검증**: "GeoJSON 경로 생성" 클릭 → 지도에 **노란 주행 경로 렌더**. 주행모드·속도·고도·카메라 컨트롤 UI 정상.

### 11. buildingcontrol — 건물모델(glb) 편집 ✅ (수정 완료)
- **스토리**: 지점 선택 → glb 3D 모델 배치 → 회전/이동/배율 편집.
- **원인**: modeler.js cross-origin import base 문제.
- **해결**: modeler.js 스크립트 태그에 `crossorigin="anonymous"` 추가.
- **검증**: 콘솔 에러 0, `buildingControl` 객체 적재, `ws3de.modeler` 로드.

---

## 2.0 가시화 (4종) — 전부 완전 동작

### 12. heatmap — Heatmap 가시화 ✅✅
- **스토리**: 버튼 클릭 → WMS 레이어를 단일이미지 히트맵으로 가시화.
- **검증**: "히트맵 단일이미지 ON" 결과 표시.

### 13. cluster — Cluster 가시화 ✅✅
- **스토리**: 버튼 클릭 → 24개 점 데이터를 군집 원으로 가시화.
- **검증**: 한강·여의도 지도 위 클러스터 마커(3·6·2···) 렌더, "총 24개 데이터 포인트 클러스터링, 상태 ON".

### 14. grid — Grid 가시화 ✅✅
- **스토리**: 버튼 클릭 → 사각 격자 3D 막대(높이=값) 가시화.
- **검증**: 색상별 3D 막대 렌더, "Grid 막대 24개 (min=35, max=90)".

### 15. hexbin — Hexbin 가시화 ✅✅
- **스토리**: 버튼 클릭 → 육각 격자 3D 기둥 가시화.
- **검증**: 육각 기둥 렌더, "Hexbin 막대 24개 (min=35, max=90)".

---

## 종합

| 등급 | 종 | 수 |
|------|-----|----|
| ✅✅ 완전 동작 | slope, route, heatmap, cluster, grid, hexbin | 6 |
| ✅ 로드 정상(클래스/모듈 적재·에러 0) | terrainvolume, profile, sunlight, sunlightrights, viewsurface, **sunlightslope, visiblearea, culheritalter, buildingcontrol** | 9 |

**결론**: **15종 전부 콘솔 에러 0 + 분석 모듈/클래스 정상 적재**. 이전에 △였던 4종(sunlightslope·visiblearea·culheritalter·buildingcontrol)은 수정으로 해결:
- modeler 의존 3종 → modeler.js에 `crossorigin="anonymous"` (map.vworld.kr의 `ACAO:*` 활용, 프록시 불필요)
- visiblearea → page.js 참조 DOM stub 3개 추가

생성 HTML만으로 전 종 해결됨(실행 환경/프록시 불필요).

## 위치 지정 (B 방식 — 15종 공통)
초기 카메라 좌표(`vw.CoordZ`)는 15종 모두 동일 리터럴이라 단일 치환으로 전 종 적용됨.
- `--address "<주소>"`: geocode(GetCoord)로 좌표 변환 후 지도 중심 이동 (최우선)
- `--center "lon,lat"`: 좌표 직접 지정
- 미지정: 샘플 기본 위치(서울 여의도) 유지

```bash
# 주소로 그 위치 경사도 분석
vworld map 3dsim --analysis slope --address "서울특별시 용산구 남산공원길 105" -o slope.html
# 좌표로 직접
vworld map 3dsim --analysis terrainvolume --center 129.16,35.16 -o tv.html
```
**실증**: 남산 주소 → 지도가 남산으로 이동 + 산악 경사도 분포(45°~ 2.22%, 25~30° 22.22% 등) 계산 확인. (`.vw_demo/shots/slope_namsan.jpeg`)

> 참고: cluster/grid/hexbin/heatmap/route는 데모 데이터가 특정 위치에 내장되어 있어, `--center`는 카메라만 이동하고 데이터는 원위치 유지됨. 지형·건물 분석류(slope/terrainvolume/profile/sunlight/visiblearea/viewsurface 등)는 사용자가 지도에서 직접 지점·영역을 지정하므로 위치 이동이 그대로 분석에 반영됨.

## 재현 방법
```bash
# 1. 15종 생성
for a in $(./target/debug/vworld map 3dsim --analysis list --raw | cut -f1); do
  ./target/debug/vworld --config skills/app/config.toml map 3dsim --analysis "$a" -o ".vw_demo/$a.html"
done
# 2. 로컬 서버
cd .vw_demo && python3 -m http.server 8765
# 3. 브라우저에서 http://localhost:8765/<type>.html 열기
```
