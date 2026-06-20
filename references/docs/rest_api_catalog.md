# VWorld 개발 가이드 13종 전수 분석 — 엔드포인트·파라미터·개발옵션

> 출처: vworld.kr/dev 개발 가이드 13개 페이지 + 동적 리스트(2ddata s002·NED dtmk) 전수 수집·파싱
> 수집일: 2026-06-16 · 국가중점데이터(NED) 115건은 별도 `national_data_catalog.md` 참조
> 용도: `api/*.rs` 모듈 구현의 권위 레퍼런스 + skill `docs/<모듈>.md` 기반

## 0. 공통 규약 (REST `req` 패밀리 전체 공통)

VWorld REST API는 **`https://api.vworld.kr/req/{service}`** 단일 패턴 + 쿼리 파라미터로 동작한다.

| 공통 파라미터 | 선택 | 설명 | 유효값 |
|----------------|------|------|--------|
| `service` | O | 요청 서비스명 | address / search / data / image / WMS / WFS … (엔드포인트별 기본값) |
| `version` | O | 서비스 버전 | 2.0(기본), WMS는 1.3.0 |
| `request` | **M** | 오퍼레이션 | 서비스별(GetCoord, search, GetFeature, GetMap …) |
| `key` | **M** | 발급 api key | — |
| `format` | O | 응답 포맷 | json(기본) / xml (이미지형은 png/jpeg/bmp) |
| `errorFormat` | O | 에러 포맷 | json / xml / image / blank |
| `crs` | O | 좌표계 | EPSG:4326(기본), 5179/5181/5186 등 |
| `callback` | O | JSONP 콜백 | format=json일 때 |
| `domain` | O | **인증키 발급 시 등록한 URL. "HTTPS·Flex 등 웹뷰어가 아닌 브라우저(=CLI) 사용 시 domain 추가"** |

- **인증**: `key=` 쿼리(전수 공통). 도메인 등록 키는 `domain=` 쿼리 또는 `Referer` 헤더.
- **HTTP 200 본문 에러**: `errorFormat` 포맷으로 에러 코드/메시지 반환 — 상태코드만으로 성공 판정 금지(설계 §2.1.1-b).
- **3계열 분류(설계 §1.1)**: 데이터형(json/xml) · 이미지형(png 등 바이트) · 타일형(REST 경로) · 렌더링형(JS).

---

## 1. 데이터 반환형 (REST → JSON/XML)

### 1.1 Geocoder — `/req/address` (일일 40,000건)
- `request=GetCoord`(지오코딩, 주소→좌표). 역지오는 동일 엔드포인트 `request=GetAddress`(좌표→주소, `point=x,y`, `type=PARCEL|ROAD|BOTH`) — 본 가이드 페이지는 GetCoord 중심.
- 핵심 파라미터: `type`(M, **PARCEL=지번 / ROAD=도로명**), `address`(M, 검색 키워드), `refine`(true기본/false 정제생략), `simple`(true/false기본 간략출력), `crs`.
- 출력: `response.result.point.{x,y}` + `refined` 주소. 에러코드표 존재.

### 1.2 검색 — `/req/search`
- `request=search`. `type`(M, **PLACE=장소 / ADDRESS=주소 / DISTRICT=행정구역 / ROAD=도로명**).
- `category`: type별 하위유형(장소=분류코드 / 주소=ROAD·PARCEL / 행정구역=L1시도·L2시군구·L3읍면동).
- 페이징: `size`(1~1000, 기본10), `page`(기본1). 공간: `bbox`(minx,miny,maxx,maxy), `crs`.
- `query`(M): 장소명/주소/도로명/행정구역명.

### 1.3 2D데이터 — `/req/data` (총 **158 데이터셋**)
- `request=GetFeature`(피처조회) / `GetFeatureType`(스키마조회).
- `data`(M): **데이터셋 ID** (예: `LP_PA_CBND_BUBUN`=연속지적도). 158종 카탈로그는 `v4dv_2ddataguide2_s003.do?svcIde=<id>` 상세 + JS-로딩 목록(구현 시 NED와 동일 방식으로 `docs/twod_data_catalog.md`에 전수 harvest 예정).
- 공간필터 `geomFilter`(M): `POINT(x y)` / `LINESTRING(...)` / `POLYGON((...))` / `BOX(...)`.
- 속성필터 `attrFilter`(O/n): `속성명:연산자:값|...` (geomFilter/attrFilter 모두 없으면 `emdCd` 읍면동코드 필수).
- `columns`(반환 컬럼 선택), `geometry`(true기본/false), `attribute`(true기본/false), `buffer`(m, 기본0), `size`(max1000)/`page`, `crs`.

### 1.4 WMS/WFS — `/req/wms`, `/req/wfs` (version 1.3.0)
- **WMS** `request=GetMap`(맵 이미지) / `GetCapabilities`(능력정의). → 이미지형.
- **WFS** `request=GetFeature`(지리객체 GML/XML) / `GetCapabilities`. → 데이터형(GML/XML, `outputFormat`으로 JSON 가능).
- `layers`(M, 쉼표구분 **최대 4개**), `styles`(1:1), `bbox`(M, xmin,ymin,xmax,ymax — **EPSG:4326 등은 축순서 예외**), `width`/`height`(M), `transparent`(TRUE/FALSE기본), `bgcolor`(0xFFFFFF), `crs`, `domain`, `exceptions`(text/xml).
- 레이어 목록은 v4apiRefer 레퍼런스 참고.

### 1.5 다운로드 카탈로그 — `/ned/dtmk/*` (NED dtmk 계열)
- `getDatasetList.do`(데이터셋 목록) / `getGidList.do`(분류 목록) / `getGidDsList.do`(분류별 데이터셋).
- 파라미터: `gid_cd`(분류체계 01~16), `ds_id`(데이터셋 id), `ds_nm`(데이터셋명), `pageNo`, `numOfRows`(max1000), `format`(xml기본/json), `key`.

---

## 2. 이미지/파일 저장형 (바이트 → 파일 저장)

### 2.1 StaticMap — `/req/image`, `request=GetMap`
- `basemap`(NONE=흰배경 / GRAPHIC=기본지도 / GRAPHIC_WHITE=백지도 / SATELLITE / HYBRID …).
- `center`(M, x,y), `crs`, `zoom`(M, **6~18**), `size`(M, width,height **최대 1024,1024**), `format`(png기본/jpeg/bmp).
- 오버레이: `layers`/`styles`(주제도), `marker`(O/n, 서브파라미터), `route`(O/n, 경로).

### 2.2 범례이미지 — `/req/image`, `request=GetLegendGraphic | GetLegendStyle`
- `layer`(M, 대상 레이어), `style`(레이어 스타일), `type`(**ALL=레이어+하위 / LAYER=레이어만 / SUB=하위만**), `format`(png/jpeg/bmp).

### 2.3 WMTS 타일 — `/req/wmts/1.0.0/{layer}/{tileMatrix}/{tileRow}/{tileCol}.{tileType}`
- **RESTful 타일 경로**(쿼리 아님). `layer`(Base/white/midnight/Hybrid/Satellite), `tileMatrix`(줌, 레이어별 6~18/6~19), `tileRow`(Y, Google index), `tileCol`(X), `tileType`(png/jpeg). `key` 쿼리.
- `GetCapabilities`로 타일매트릭스셋 조회 가능.

### 2.4 TMS 타일 — `/req/tms/1.0.0/...`
- WMTS와 동일 레이어/줌 체계이나 **TMS 좌표 스킴(Y축 반전: 원점이 좌하단)**. WMTS↔TMS 타일Y 변환 주의.

### 2.5 벡터지도(타일) — `/req/wmts/vector/getTile/{...}`, `getStyle/{...}`
- `getTile`: **MVT(Mapbox Vector Tile)** 반환. `layer`(poi/traffic / 배경 Base), `tileMatrix`(6~19), `tileRow`(X), `tileCol`(Y), `key`.
- `getStyle`: 벡터 스타일 JSON(Mapbox GL style). → CLI는 타일 바이트 저장 + 스타일 JSON 반환.

---

## 3. 브라우저 JS 렌더링형 (CLI 직접 렌더 불가 → URL/HTML/설정 생성)

### 3.1 2D지도 — `map.vworld.kr/js/vworldMapInit.js.do`
- OpenLayers 기반 2D 지도 JS API. `<script src="…/vworldMapInit.js.do?apiKey=<KEY>&domain=<URL>">` 포함 후 JS 객체로 지도 초기화.
- CLI 산출물: 위 스크립트 include + 초기화 스니펫이 담긴 **HTML 파일 / 지도 URL / 설정 JSON** 생성(직접 렌더 Non-Goal).

### 3.2 웹지엘 3D지도 — `map.vworld.kr/js/webglMapInit.js.do`
- WebGL(Cesium 계열) 3D 지구본. 동일하게 스크립트 include + 3D 초기화. CLI는 HTML/URL/설정 생성.

### 3.3 3D분석·시뮬레이션 — `webglMapInit.js.do` + tool3d 라이브러리
- 3D 위에 분석 도구 로드: `terrainVolume`(지형 체적), `heatmap`, `cluster`, `customVerticalBarPrimitive`(수직 막대) 등 `…/js/dtkmap/tool3d/libapis/*` ESM/JS 모듈.
- CLI는 선택한 분석 모듈을 포함한 HTML/설정 생성(렌더·계산은 브라우저). Non-Goal: CLI 직접 분석 실행.

---

## 4. 모듈↔엔드포인트↔설계분류 매핑 요약

| api 모듈 | 엔드포인트 | request/오퍼레이션 | 설계 §1.1 분류 |
|----------|------------|---------------------|----------------|
| `geocode.rs` | `/req/address` | GetCoord / GetAddress | 데이터형 |
| `search.rs` | `/req/search` | search | 데이터형 |
| `data.rs` | `/req/data` | GetFeature / GetFeatureType | 데이터형 (158 데이터셋) |
| `wms_wfs.rs` | `/req/wms` · `/req/wfs` | GetMap·GetCapabilities / GetFeature | 이미지형(WMS) + 데이터형(WFS) |
| `catalog.rs` | `/ned/dtmk/*` | getDatasetList / getGidList / getGidDsList | 데이터형 |
| `national_data.rs` | `/ned/{wms\|wfs\|data}/*` | 115 오퍼레이션 | 데이터형+이미지형 |
| `staticmap.rs` | `/req/image` | GetMap | 이미지형 |
| `legend.rs` | `/req/image` | GetLegendGraphic / GetLegendStyle | 이미지형 |
| `tile.rs` | `/req/wmts/1.0.0/…` · `/req/tms/1.0.0/…` · `/req/wmts/vector/…` | GetTile(REST 경로) / getStyle | 이미지형(타일) |
| `map_embed.rs` | `map.vworld.kr/js/{vworldMapInit\|webglMapInit}.js.do` | (JS include) | 렌더링형 |

> **신규 함의**: 벡터타일(MVT)·WMTS·TMS는 `tile.rs` 하나로 통합 가능(공통 타일좌표 모델). 검색·2D데이터·지오코더는 `size/page` 페이징 공통. `domain=` 쿼리는 전 REST 공통 인증 보조 경로.
