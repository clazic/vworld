---
name: vworld
description: VWorld OpenAPI(지오코딩·검색·2D데이터·국가중점데이터·WMS/WFS·타일·StaticMap·지도임베드)를 호출하는 자기완결 Rust CLI. "이 주소 좌표 알려줘", "용도지역 조회", "이 좌표 주소 뭐야", "지적도/공시지가/건물정보 같은 공간데이터", "지도 이미지 저장" 류 한국 공간정보 질의에 사용.
---

# VWorld CLI 스킬

VWorld OpenAPI를 도구로 사용하는 스킬. 바이너리는 `app/vworld`(자기완결), 키는 `app/config.toml`.

## 사용 전 필수 (자기학습 루프)

1. **작업 전** `LEARNINGS.md`를 먼저 읽어라(과거 함정·성공 쿼리).
2. **작업 후** 새로 배운 실패·함정·성공 쿼리를 `LEARNINGS.md`에 5요소 형식으로 1줄+ append하라.

## 바이너리 호출

```
app/vworld [전역옵션] <명령> [인자]
```

전역옵션: `--pretty`(들여쓰기) `--raw`(원응답) `--concurrency N` `--timing` `--referer <URL>` `--config <path>`.
출력은 **JSON 기본**(stdout). 이미지형은 파일 저장 + 경로 JSON 보고.

## 자연어 의도 → 명령 매핑

| 사용자 의도 | 명령 |
|-------------|------|
| "이 주소 좌표 알려줘" | `vworld geocode "<주소>" --type ROAD` (지번이면 `--type PARCEL`) |
| "이 좌표 주소 뭐야" | `vworld geocode "<x,y>" --reverse --type BOTH` |
| "장소/건물 검색" | `vworld search "<키워드>" --type PLACE` |
| "행정구역/도로명 검색" | `vworld search "<키워드>" --type DISTRICT` (또는 ROAD) |
| "이 영역 지적도/공간데이터" | `vworld data <데이터셋ID> --geom-filter "BOX(...)"` (예: `LP_PA_CBND_BUBUN`) |
| "건축물연령/공시지가/용도지역 등 국가중점데이터" | `vworld ned <오퍼레이션> --pnu <필지번호>` (목록: `vworld ned --list`) |
| "OO동 **전체 필지**의 공시지가/속성" | `vworld ned <WFS오퍼레이션> --pnu <법정동8자리> --all` (1000 cap 자동 우회) |
| "OO동을 **행정동별로** 공시지가 비교" | `vworld ned getIndvdLandPriceWFS --pnu <법정동8자리> --by-hjd` (역지오 행정동 분류+통계, 자동 재처리) |
| "여러 PNU 한꺼번에" | `vworld ned <data오퍼레이션> --input pnus.txt --concurrency 6` |
| "WMS 레이어 능력/맵" | `vworld wms --request GetCapabilities` / `--request GetMap --layers ... --bbox ...` |
| "WFS 지리객체" | `vworld wfs --request GetFeature --typename ... --bbox ...` |
| "다운로드 카탈로그" | `vworld catalog datasets --gid-cd 01` |
| "지도 이미지 저장" | `vworld staticmap "<x,y>" --zoom 14 --size 512,512 -o map.png` |
| "범례 이미지" | `vworld legend <레이어> -o legend.png` |
| "범례 SLD 스타일" | `vworld legend <레이어> --sld -o legend.sld.xml` (GetLegendStyle, `--sld`는 부울 플래그·출력 경로는 `-o`) |
| "배경지도 타일" | `vworld tile wmts --layer Base --z 14 --row <Y> --col <X> -o tile.png` |
| "WMTS 주제도 목록(해외위성 등 시계열)" | `vworld tile wmts-themes --category cities --year 2025 --city Oslo --z 11 --row 1086 --col 596 -o oslo.png` |
| "WMTS 능력문서(메타데이터)" | `vworld tile wmts-capabilities -o WMTSCapabilities.xml` |
| "지도 띄우는 HTML" | `vworld map 2d --center 127,37.5 -o map.html` (3d/3dsim 가능) |
| "2D 데이터지도(벡터/마커/차트/주제도)" | `vworld map ol\|marker\|chart\|theme\|text ...` (OpenLayers 2D — 아래 섹션) |
| "GeoJSON/폴리곤 지도에 표시" | `vworld map ol --geojson f.geojson -o m.html` / `--polygon "lon,lat;…"` |
| "여러 주소 한꺼번에" | `vworld geocode --input addrs.txt --concurrency 4` |
| "2D 데이터레이어 158종 탐색" | `vworld data layers` (전체 목록; `--search <키워드>` / `--cat <카테고리>` / `--geom <타입>` 필터) |
| "특정 2D 레이어 속성 확인" | `vworld data describe <데이터ID>` (속성표·단일검색키·샘플URL) |
| "연속지적도 DXF 내보내기" | `vworld ned getCtnlgsSpceWFS --address "<주소>" --radius 1000 --dxf parcels.dxf` (기본 EPSG:5187, `--dxf <경로>` 는 경로 인자) |
| "연속지적도 SHP 내보내기" | `vworld ned getCtnlgsSpceWFS --address "<주소>" --radius 1000 --shp parcels.shp` (속성포함 5종 생성, `--shp <경로>` 는 경로 인자) |

## 키 관리

```
vworld config add-key <KEY> [--alias main] [--referer https://example.com]
vworld config list-keys      # 마스킹 출력
vworld config remove-key <KEY|index>
vworld config test-keys      # 실 호출 유효성(도메인불일치 해결 가이드 포함)
vworld config path
```

- 등록된 모든 키가 동시성 키 풀에 자동 편입(`--concurrency`로 병렬 가속).
- 도메인 등록 키는 `--referer`(또는 config `referer`)로 `domain=` 쿼리·Referer 헤더 주입. CLI는 웹뷰어가 아니므로 도메인 등록 키면 referer 필수일 수 있음.

## 레퍼런스 문서 (docs/)

- `docs/rest_api_catalog.md` — 13종 REST 엔드포인트·파라미터·옵션 전수.
- `docs/national_data_catalog.md` — 국가중점데이터(NED) 115 오퍼레이션 전수.
- `docs/USAGE.md` — 명령별 상세 사용법·입출력 예시·함정.

## 함정 (요약 — 상세는 LEARNINGS.md)

- **주소 유형**: 도로명=ROAD, 지번=PARCEL. 틀리면 결과 없음(빈 result).
- **bbox 축순서**: `EPSG:4326`은 `(ymin,xmin,ymax,xmax)`로 위경도 반전.
- **NED WMS 계열**은 이미지(타일/staticmap 경로). 데이터형은 WFS/속성(data) 계열.
- **타일 좌표**: WMTS `--row`=Y, `--col`=X. TMS는 CLI가 Y축 반전 자동 처리.
- **HTTP 200 본문 에러**: CLI가 자동 검사. "결과 없음"은 정상(빈 결과)로 처리됨.

## 통합 지오코더 (geocoder) · 자동 지오코딩 (geocode)

- `vworld geocoder "<주소 또는 x,y>"` → **좌표·지번·도로명을 한 번에**(apis.vworld.kr). 입력 형식 자동 감지(주소↔좌표).
- `vworld geocode "<주소>"` — `--type auto`가 기본(도로명→지번 자동 폴백). 좌표("x,y") 입력 시 자동 역지오코딩(`--reverse` 불필요). `--type ROAD|PARCEL` 수동 지정도 가능.

## 3D 분석·시뮬레이션 (map 3dsim --analysis) — 15종

`vworld map 3dsim --analysis <type> --address "<주소>"`(또는 `--center lon,lat`) `-o out.html`
- 목록: `vworld map 3dsim --analysis list`
- 종류: slope 경사도 · terrainvolume 토공량 · profile 지형단면 · sunlight 일조량 · sunlightrights 일조권 · sunlightslope 일조사선제한 · visiblearea 가시면적 · viewsurface 시곡면 · culheritalter 문화재현상변경 · route 드론·차량주행 · buildingcontrol 건물편집 · heatmap · cluster · grid · hexbin
- 파라미터(위치·옵션·지도 인터랙션) 명세: `docs/3dsim_analysis_params.md`
- 위치는 `--address`(지번/도로명 자동) 또는 `--center lon,lat` 주입. 공식 샘플에 토스 디자인 + 큰 지도가 적용됨.

### 결과값 자동 추출 (중요)
CLI는 **분석 HTML 생성만** 한다 — 3D 분석은 브라우저(Cesium/WebGL) 라이브러리에서만 계산되어 CLI가 직접 값을 내지 못한다. 결과값(경사도 분포·성토/절토량 등)을 얻으려면 헤드리스 브라우저가 필요하며, **Playwright는 이 스킬에 포함되지 않는다**(사용 환경의 기능):
1. **Claude Code + Playwright MCP 환경**: AI가 생성 HTML을 열어 분석을 트리거하고 결과를 추출 — 지점형=캔버스 클릭, 영역형=좌클릭 정점들 + **우클릭으로 폴리곤 완료**(합성 이벤트는 Cesium이 무시 → trusted 입력 필요), 카메라 고도 ≤2km.
2. **직접**: 생성 HTML을 브라우저로 열어 수동으로 지점/영역 지정.
3. **`--execute` 빌드(옵션·미구현 기본)**: Chrome 의존이라 자기완결 배포엔 미포함.
- 값 반환 대상: slope·terrainvolume·profile·sunlight·sunlightrights·visiblearea·viewsurface. **route·buildingcontrol·heatmap·cluster·grid·hexbin은 반환할 "값"이 없음**(시뮬레이션/시각화).

## 2D 데이터레이어 지도 (map ol/marker/chart/theme/text)

vworld 2D지도 API 2.0(OpenLayers 3.10.1 기반 `vw.ol3.*`) 코드샘플을 CLI로 반영한 **OpenLayers 2D 데이터레이어** 명령군. 기존 `map 2d`(WebGL 3D엔진의 평면 모드)와 **별개**다 — 신규 키 `map ol`은 벡터·마커·차트·주제도 등 데이터 시각화 데모용.

> **2d vs ol 구분**: `map 2d` = 3D엔진(Cesium/WebGL) 평면 모드 / `map ol` = OpenLayers 데이터레이어. 데이터(GeoJSON/마커/차트/주제도)를 얹으려면 **`map ol` 계열**을 쓴다.

| 명령 | 용도 | 예시 |
|------|------|------|
| `map ol` | 기본 2D 지도 + 벡터(폴리곤/GeoJSON) + 컨트롤 | `vworld map ol --center 127,37.5 --zoom 13 --basemap PHOTO --geojson f.geojson -o m.html` |
| `map marker` | 마커 + 팝업 | `vworld map marker --points markers.json -o m.html` |
| `map chart` | 위치 기반 차트(막대/누적/파이) + 범례 | `vworld map chart --type bar --data chart.json [--group] -o m.html` |
| `map theme` | WMS 주제도(named layer) + 토글 | `vworld map theme --layers "도시지역:LT_C_UQ111,관리지역:LT_C_UQ112" -o m.html` |
| `map text` | 대량 포인트(TEXTLayer 클러스터링) | `vworld map text --file points.txt --epsg EPSG:4326 --distance 40 -o m.html` |
| `map controller` | 2D/3D 전환 지도(vw.MapController) | `vworld map controller --center 127,37.5 -o m.html` |

### 공통 옵션
- `--center lon,lat`(기본 127.0,37.5) / `--address "<주소>"`(geocode, `map ol`에서 --center보다 우선) / `--zoom`(기본 11).
- `--basemap GRAPHIC|GRAPHIC_WHITE|GRAPHIC_NIGHT|PHOTO|PHOTO_HYBRID`(기본 GRAPHIC).
- 입력 좌표는 **EPSG:4326(lon,lat)** 기본 — JS측 `ol.proj`가 내부 변환.
- 모든 산출물에 **토스 디자인 + 절대경로(https://)** 적용.

### map ol 세부
- **벡터 입력**: `--polygon "lon,lat;lon,lat;…"`(인라인 폴리곤, 최소 3정점) / `--geojson <file>`(FeatureCollection — 폴리곤/포인트/라인). 토스 #3182f6 스타일, 자동 extent 맞춤.
- **KML**: `--kml <https URL>`(외부 KML, 절대경로 https만 — CORS).
- **컨트롤 플래그**: `--zoom-control`(PanZoomBar) / `--basemap-switch`(배경 전환 버튼) / `--popup`(클릭 좌표 팝업).
- **추가 컨트롤**: `--overview`(미니맵) / `--toolbar`(거리·면적 측정 등 11종) / `--route "lon,lat;…"`(사전 경로 폴리라인).

### 입력 파일 스키마
- **markers.json**: `[{x, y, epsg?, title, contents, iconUrl?, text?, attr?}]` — `epsg` 기본 `EPSG:4326`(즉 x=lon, y=lat).
- **chart.json**: `[{pos:[lon,lat], title, size?:[w,h], radius?(pie), styles:[{color,label,legendLabel?}], values:[…]}]` — 누적막대(stackedbar)는 `values:[[…],[…]]` 중첩.
- **points.txt**(text): vworld TEXT 포맷 `lon⇥lat⇥title⇥desc⇥iconSize`(탭 구분, 헤더 1줄). **자기완결 상한 500줄**(초과 시 거부 — 분할 필요).

### 주의 (단계1·2 검증으로 확인된 동작)
- 생성 HTML은 `window.addEventListener('load')` 안에서 지도를 초기화한다 — vw.ol3.Map은 런타임 로드 후 초기화해야 벡터·컨트롤이 렌더된다.
- 벡터 영역 맞춤은 `setCenter`+`setResolution`(vwFit)으로 처리(vw.ol3.Map의 `fit()`이 maxZoom 무시·과확대하는 문제 우회).
- 기존 Leaflet WFS/GeoJSON 뷰어(`wfs --viewer`)와 ol3 `map ol --geojson`은 **공존**(둘 다 GeoJSON 표시 가능).
- vworld 로고·"연속지적도…참고용" 안내문구는 모든 생성 HTML에서 CSS로 숨김 처리(`.vw-logo`/`.vw-notice`).
- 계획·검증 상세: `plan/2026-06-18-09:30:53-2dmap-19samples.md`.

## 데이터 자원 (skills/data)

### 빌드타임 임베드 카탈로그 (런타임 파일 불요)

| 파일 | 설명 |
|------|------|
| `ned_catalog.tsv` | NED 오퍼레이션 카탈로그 (build.rs codegen으로 바이너리 임베드) |
| `ned_params.tsv` | NED 파라미터 정의 (build.rs codegen으로 바이너리 임베드) |
| `twod_catalog.tsv` | 2D 데이터레이어 158종 카탈로그 (build.rs codegen으로 바이너리 임베드) |
| `twod_attrs.tsv` | 2D 레이어 속성 정의 (build.rs codegen으로 바이너리 임베드) |
| `twod_seed.tsv` | 2D 레이어 시드 데이터 (build.rs codegen으로 바이너리 임베드) |

이 TSV 파일들은 `build.rs`에서 `include_str!` / codegen으로 **빌드타임에 바이너리에 임베드**된다. 런타임에 파일을 참조하지 않으므로 사용자가 별도로 신경 쓸 필요 없다. 코어(바이너리 + 임베드 tsv)는 **자기완결**이다.

### vworld.sqlite (132MB) — opt-in 런타임 자원

`vworld.sqlite`는 `--by-hjd` 행정동별 고속 처리를 위한 **선택적(opt-in) 런타임 DB**다. 코어 자기완결 범위 밖의 자원이며 `skills/app`에 동봉하지 않고 `skills/data`에 유지한다.

- **디폴트 경로 없음** — `--hjd-db` 없이도 `--by-hjd`는 역지오코딩 폴백으로 정상 동작(sqlite 불요).
- **고속화 원하면 부트스트랩 1회 필요**:

```bash
# 1) 행정동 경계 SHP로 DB 생성
vworld hjd-db build --shp <행정동경계.shp> --db skills/data/vworld.sqlite

# 2) --hjd-db 경로 인자로 명시 참조
vworld ned getIndvdLandPriceWFS --pnu <법정동8자리> --by-hjd --hjd-db skills/data/vworld.sqlite
```

- `--hjd-db <path>`는 **경로 인자** — 자동 참조 안 됨, 반드시 명시.
- sqlite 미사용 시: `--by-hjd`가 역지오코딩 폴백으로 동작(느리지만 정상).

## 2D 데이터레이어 탐색 워크플로

158종 레이어를 발견 → 속성 확인 → 실제 조회하는 3단계 흐름.

```bash
# 1단계: 발견 — 158종 레이어 목록 조회
vworld data layers
vworld data layers --search "지적"          # 키워드 필터
vworld data layers --cat "토지"             # 카테고리 필터
vworld data layers --geom polygon           # 지오메트리 타입 필터

# 2단계: 속성 확인 — 레이어 ID의 속성표·샘플URL 조회
vworld data describe LP_PA_CBND_BUBUN

# 3단계: 실제 조회 — GetFeature 호출
vworld data LP_PA_CBND_BUBUN --geom-filter "BOX(126.9,37.4,127.1,37.6)"
vworld data LP_PA_CBND_BUBUN --attr-filter "ADDR_CD='1168010100'"
```

> **팁**: `data layers` 출력의 데이터ID를 `data describe <id>`에 그대로 넣으면 단일검색키·속성표·샘플 URL을 한 번에 확인할 수 있다.
