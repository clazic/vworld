# VWorld CLI 사용설명서 — 전 명령 레퍼런스

VWorld(국가공간정보포털) OpenAPI를 감싼 자기완결 단일 바이너리 CLI. 이 문서는 **모든 명령·서브커맨드·옵션**의 상세 사용법이다. (v0.2.0 기준, `--help` 실측 검증)

- 설치·인증키 발급: `README.md` 참조
- 자연어 의도 → 명령 매핑, 함정 요약: `SKILL.md` 참조
- 출력 규약: **데이터형은 JSON(stdout)**, 이미지형은 파일 저장 + 경로 JSON 보고. 에러는 stderr에 JSON 친화 형식.

## 명령 전체 목록

| 명령 | 별칭 | 설명 | 키 필요 |
|------|------|------|:---:|
| `geocode` | `geo` | 지오코딩/역지오코딩 (/req/address) | O |
| `geocoder` | | 통합 지오코더 — 좌표·지번·도로명 한 번에 | O |
| `search` | `s` | 장소·주소·행정구역·도로명 검색 (/req/search) | O |
| `data` | | 2D데이터 158종 — layers/describe/fetch/join | fetch만 O |
| `ned` | | 국가중점데이터 115 오퍼레이션 (/ned/*) | O |
| `wms` | | WMS GetMap/GetCapabilities | O |
| `wfs` | | WFS GetFeature (+HTML 뷰어) | O |
| `catalog` | | 다운로드 카탈로그 (/ned/dtmk/*) | O |
| `staticmap` | `static` | 정적 지도 이미지 (/req/image) | O |
| `legend` | | 범례 이미지 / SLD 스타일 XML | O |
| `tile` | | WMTS/TMS/벡터 타일 | O |
| `map` | | 지도 임베드 HTML 생성 (2D/3D/분석/데이터레이어) | O |
| `batch` | | 다건 배치 실행 (현재 geocode) | O |
| `hjd-db` | | 행정동 경계 SQLite 구축·조회 (오프라인) | X |
| `config` | | 인증키·설정 관리 | X |
| `update` | | 자가 업데이트 (GitHub Releases) | X |

## 전역 옵션 (모든 명령 공통)

```
vworld [전역옵션] <명령> [인자]

--concurrency N   동시 in-flight 요청 상한(워커 수). 미지정 시 max(키수, 2)
--pretty          JSON 들여쓰기 출력
--raw             원응답 그대로 출력(가공 없이)
--timing          수행 시간 측정 정보 출력
--referer <URL>   도메인 등록 키용 Referer/domain 일회성 오버라이드
--config <path>   설정파일 경로 지정. 기본 ~/.vworld/config.toml (Windows: %USERPROFILE%\.vworld\config.toml)
```

- 등록된 모든 키가 동시성 키 풀에 자동 편입 — `--concurrency`로 병렬 가속.
- 도메인 등록 키는 `--referer`(또는 config `referer`) 필수일 수 있음. 무도메인 키는 불필요.

---

## geocode — 지오코딩/역지오코딩 (`/req/address`)

```
vworld geocode [OPTIONS] [QUERY]
```

| 옵션 | 설명 |
|------|------|
| `QUERY` | 주소(지오) 또는 좌표 `"x,y"`(역지오) |
| `--type auto\|ROAD\|PARCEL` | 주소 유형. 기본 `auto` = 도로명→지번 자동 판별·폴백 |
| `--reverse` | 역지오코딩(좌표→주소) 명시. 좌표 형식 입력 시 자동 감지되므로 보통 생략 가능 |
| `--crs <CRS>` | 응답 좌표계 (기본 EPSG:4326) |
| `--input <file>` | 다건 입력 파일(줄당 1건, `#` 주석/빈줄 스킵) — 배치 |

```bash
vworld geocode "세종대로 110"                       # 주소 → 좌표 (auto)
vworld geocode "서울특별시 중구 세종대로 110" --type ROAD
vworld geocode "126.978,37.566"                     # 좌표 → 주소 (자동 역지오)
vworld geocode --input addrs.txt --concurrency 4    # 배치
```

- 출력: `response.result.point.{x,y}` + `refined` 정제주소.
- **함정**: `--type` 수동 지정 시 유형 불일치(도로명↔지번)면 빈 결과. 모르면 `auto`.

## geocoder — 통합 지오코더 (apis.vworld.kr)

```
vworld geocoder [OPTIONS] <QUERY>
```

주소(지번/도로명) 또는 좌표 `"x,y"`를 자동 감지해 **좌표·지번·도로명을 한 번에** 반환.

| 옵션 | 설명 |
|------|------|
| `--epsg <EPSG>` | 응답 좌표계 (기본 epsg:4326) |

```bash
vworld geocoder "경상남도 고성군 하이면 덕명리 420-1"
vworld geocoder "127.0,37.5"
```

## search — 검색 (`/req/search`)

```
vworld search [OPTIONS] <QUERY>
```

| 옵션 | 설명 |
|------|------|
| `--type PLACE\|ADDRESS\|DISTRICT\|ROAD` | 검색 대상 (기본 PLACE) |
| `--category <cat>` | 카테고리. **ADDRESS는 `ROAD\|PARCEL`, DISTRICT는 `L1~L4` 필수**(없으면 PARAM_REQUIRED 에러, 실측). PLACE/ROAD는 선택 |
| `--bbox minx,miny,maxx,maxy` | 검색 영역 제한 |
| `--size N` / `--page N` | 페이지 크기(1~1000) / 페이지 |
| `--crs <CRS>` | 좌표계 (기본 EPSG:4326) |

```bash
vworld search "광화문" --type PLACE
vworld search "판교로 344" --type ROAD --size 20 --page 1
vworld search "종로구" --type DISTRICT --category L2        # L1 시도 / L2 시군구 / L3 읍면동 / L4 리
vworld search "태평로1가 31" --type ADDRESS --category PARCEL
```

## data — 2D데이터레이어 158종 (`/req/data`)

`data <데이터ID>` 위치인자 조회와 서브커맨드(`layers`/`describe`/`fetch`/`join`)가 공존한다.

### data layers — 레이어 목록 (오프라인, 키 불요)

```bash
vworld data layers                       # 전체 158종
vworld data layers --search "지적"       # 키워드 부분일치(data_id·name·cat, 대소문자 무시)
vworld data layers --cat "토지"          # 카테고리 일치
vworld data layers --geom polygon        # geometry 타입(polygon|line|point)
```

### data describe — 레이어 상세 (오프라인, 키 불요)

```bash
vworld data describe LP_PA_CBND_BUBUN    # 속성표·단일검색키·샘플URL. 소문자 입력 허용
```

### data <ID> / data fetch — GetFeature 조회

```
vworld data <데이터ID> [OPTIONS]         # = vworld data fetch <데이터ID>
```

| 옵션 | 설명 |
|------|------|
| `--geom-filter <expr>` | 공간 필터: `POINT(x y)` / `POLYGON((...))` / `BOX(minx,miny,maxx,maxy)`. **EPSG 접미사 넣으면 INVALID_RANGE**(실측) — 좌표는 4326으로 해석. **BOX/POLYGON 요청면적 10km² 이내**(서버 제한, 실측) |
| `--attr-filter <expr>` | 속성 필터: `속성:연산자:값\|...` (예: `pnu:=:1114010300100310000`) |
| `--emd-cd <code>` | 읍면동코드. help상 geom/attr 부재 시 대체 수단이지만 **실측상 단독 사용은 INVALID_RANGE로 거부되는 데이터셋이 있음**(예: LP_PA_CBND_BUBUN) — geom/attr 필터 사용 권장 |
| `--columns <c1,c2>` | 반환 컬럼 제한 |
| `--size` / `--page` | 페이징 |
| `--crs <CRS>` | 좌표계 (기본 EPSG:4326) |

```bash
vworld data LP_PA_CBND_BUBUN --geom-filter "POINT(126.978 37.566)"      # 좌표 → 필지(pnu 획득)
vworld data LP_PA_CBND_BUBUN --geom-filter "BOX(126.977,37.565,126.979,37.567)"
vworld data LP_PA_CBND_BUBUN --attr-filter "pnu:=:1114010300100310000"
```

> **주소→필지 속성 표준 체인**: `geocode`(좌표) → `data LP_PA_CBND_BUBUN --geom-filter "POINT(x y)"`(properties.pnu) → `ned <op> --pnu <PNU>`.

**탐색 워크플로**: `layers`(발견) → `describe`(속성 확인) → `data <ID>`(조회).

### data join — 통계 JSON을 GeoJSON에 병합 (오프라인)

```
vworld data join --geojson <경계.geojson> --table <통계.json> \
  --table-key <통계측키> --table-value <값필드> [--on adm_cd] [--as <속성명>] -o out.geojson
```

| 옵션 | 설명 |
|------|------|
| `--geojson` | 경계 GeoJSON(EPSG:4326 FeatureCollection) |
| `--table` | 통계 JSON 배열 파일 |
| `--on <key>` | GeoJSON properties 측 조인 키 (기본 `adm_cd`) |
| `--table-key` / `--table-value` | 통계 측 조인 키 / 가져올 값 필드 |
| `--as <name>` | 주입할 properties 이름 (기본 = table-value) |
| `--name-tail` | 이름 조인 폴백 — 경계측 값의 마지막 공백토큰만 비교(예: "서울특별시 종로구 사직동" ↔ "사직동"). 같은 시군구 범위에서만 안전 |

- 매칭/미매칭 카운트를 JSON으로 보고(unmatched>0이면 키·연도 점검). 미매칭 feature는 no-data 처리.
- adm_cd 자릿수(시도2/시군구5/행정동8)가 양측 일치해야 함. KOSIS C1 ↔ SGIS adm_cd는 무가공 일치.

## ned — 국가중점데이터 115 오퍼레이션 (`/ned/{wms|wfs|data}`)

```
vworld ned [OPTIONS] [OPERATION]
```

```bash
vworld ned --list                                   # 115 오퍼레이션 목록
vworld ned getBuildingAge --pnu 1111018300101970001 # 단건 조회
vworld ned getIndvdLandPriceWFS --params            # 해당 오퍼레이션의 요청변수 목록
```

### 공통·데이터 옵션

| 옵션 | 설명 |
|------|------|
| `--list` | 레지스트리 목록 출력 |
| `--pnu <PNU>` | 필지고유번호(19자리). `--all`/`--by-hjd`에서는 법정동 8자리 접두 |
| `--bbox <bbox>` | bbox (WMS/WFS) |
| `--param k=v` | 임의 파라미터 패스스루(반복 가능). key/domain은 거부 |
| `--params` | 요청변수 **목록 조회 전용**(값 전달 `--param`과 다름) |
| `--input <file>` | PNU 목록 파일(줄당 1건) — 병렬 배치(data 속성 계열). `index` 순서 보존 |
| `--crs <CRS>` | 좌표계(WMS/WFS 단건). 기본 **EPSG:5187**(동부원점). 중부 5186, 서부 5185, 위경도 4326 |

### WFS 전수 수집·집계 옵션

| 옵션 | 설명 |
|------|------|
| `--all` | **전수 수집** — `--pnu <법정동 8자리>`(시도2+시군구3+읍면동3) 범위를 PNU 접두 적응분할로 maxFeatures 1000 cap 우회(무효 접두 가지치기·dedup) |
| `--by-hjd` | **행정동별 집계** — 법정동 전수 수집 → 필지 대표점을 행정동으로 분류 → 수치필드 통계(평균·중앙값·Q1·Q3·최저·최고). 내부 EPSG:4326 고정(`--crs` 무시) |
| `--value-field <f>` | `--by-hjd` 집계 수치 필드 (기본 `pblntf_pclnd`=개별공시지가) |
| `--hjd-grid <N>` | 격자 분류 정밀도(좌표 소수 자릿수, 기본 3≈100m). 인접 필지를 격자로 묶어 격자당 1회만 역지오 → 호출 수십~수백 배 절감. 4≈10m(정밀·느림), 6이면 사실상 필지별 |
| `--hjd-shp <path>` | 행정동경계 SHP(EPSG:5186, `BND_ADM_DONG_PG.shp`) point-in-polygon 분류 — 역지오 0회·즉시·정확·오프라인 |
| `--hjd-db <path>` | `hjd-db build`로 적재한 SQLite 기반 분류 — SHP보다 빠름(권장). **경로 인자, 자동 참조 안 됨** |

```bash
# 법정동 전체 필지 공시지가 (1000 cap 자동 우회)
vworld ned getIndvdLandPriceWFS --pnu 31140104 --all

# 행정동별 집계 — 역지오 분류(인터넷 필요) 또는 DB 기반(권장)
vworld ned getIndvdLandPriceWFS --pnu 26500101 --by-hjd
vworld ned getIndvdLandPriceWFS --pnu 26500101 --by-hjd --hjd-db hjd.sqlite

# PNU 목록 병렬 배치
vworld ned getIndvdLandPriceAttr --input pnus.txt --concurrency 6
```

- `--by-hjd`는 429 실패분 자동 재처리(동시성 자동 하향, 최대 6라운드), 502/연결끊김 재시도. 출력에 `격자수_역지오호출`·`커버리지`·`비대상_도로하천등`·`미해결에러` 포함.
- sqlite/SHP 미사용 시 역지오코딩 폴백으로 정상 동작(느리지만 결과 동일).

### WMS 이미지 옵션 (WMS 계열 오퍼레이션 전용)

| 옵션 | 설명 |
|------|------|
| `--width` / `--height` | 이미지 크기(픽셀, 기본 512) |
| `--img-format` | 이미지 포맷 (기본 image/png) |
| `--transparent` | 투명 배경 |
| `-o, --output <path>` | 저장 경로 (기본 `ned_<op>.png`) |

### 주소 반경 수집·내보내기 옵션 (WFS 계열 전용)

| 옵션 | 설명 |
|------|------|
| `--address "<주소>"` | 지오코딩 중심 주소 — 반경 bbox 격자수집 |
| `--radius <m>` | 반경(미터, 기본 1000). 정사각 bbox 반폭 |
| `--dxf <path>` | 수집 결과를 DXF 파일로 저장 |
| `--encoding <enc>` | DXF 텍스트 인코딩 (기본 cp949) |
| `--symbol-scale <N>` | DXF 심볼/문자 도면 스케일 분모 (기본 1000 = 1:1000) |
| `--shp <path>` | 수집 결과를 Shapefile로 저장 — .shp/.shx/.dbf/.prj/.cpg 5종 생성 |

```bash
# 연속지적도 DXF / SHP 내보내기 (기본 EPSG:5187)
vworld ned getCtnlgsSpceWFS --address "남산공원길 105" --radius 500 --dxf parcels.dxf
vworld ned getCtnlgsSpceWFS --address "남산공원길 105" --radius 500 --shp parcels.shp
```

- **함정**: NED **WMS 계열은 이미지**(타일/staticmap 경로), 데이터형은 WFS/속성(data) 계열.

## wms — WMS (`/req/wms`)

```
vworld wms [OPTIONS]
```

| 옵션 | 설명 |
|------|------|
| `--request GetCapabilities\|GetMap` | 오퍼레이션 (기본 GetCapabilities) |
| `--layers <layers>` | 레이어 (GetMap) |
| `--bbox <bbox>` | 영역 |
| `--crs <CRS>` | 좌표계(기본 EPSG:4326). **주의**: 4326·5185~5188은 bbox가 `(ymin,xmin,ymax,xmax)` = 위도,경도 순(WMS 1.3.0) |
| `--styles <styles>` | 레이어별 스타일 (생략 시 기본) |
| `--width` / `--height` | 이미지 크기 |
| `--format <fmt>` | 이미지 포맷 (기본 image/png) |
| `--transparent` | 투명 배경 |
| `-o, --output <path>` | 이미지 저장 경로 — **GetMap은 필수** |

```bash
vworld wms --request GetCapabilities
vworld wms --request GetMap --layers lt_c_uq111 --bbox 37.5,126.9,37.6,127.1 --width 512 --height 512 -o uq.png
```

## wfs — WFS (`/req/wfs`)

```
vworld wfs [OPTIONS]
```

| 옵션 | 설명 |
|------|------|
| `--request <req>` | 기본 GetFeature |
| `--typename <layer>` | 피처타입(레이어) |
| `--bbox <bbox>` | 영역 (EPSG:4326 축반전 주의) |
| `--pnu <PNU>` | 필지 필터 |
| `--max-features N` | 최대 피처 수 |
| `--crs <CRS>` | 좌표계 (기본 EPSG:4326) |
| `-o, --output <path>` | **HTML 뷰어로 저장**(토스 디자인 지도에 피처 렌더). 미지정 시 GeoJSON/JSON stdout |

```bash
vworld wfs --request GetFeature --typename lp_pa_cbnd_bubun --bbox 37.55,126.97,37.57,126.99 --max-features 100
vworld wfs --request GetFeature --typename <레이어> --bbox <...> -o viewer.html   # HTML 뷰어
```

- `output=application/json` 자동 부착(JSON 정규화).

## catalog — 다운로드 카탈로그 (`/ned/dtmk/*`)

```
vworld catalog [OPTIONS] [OP]        # OP: datasets(기본) | gids | gid-datasets
```

| 옵션 | 설명 |
|------|------|
| `--gid-cd <code>` | 분류 코드 |
| `--ds-id <id>` | 데이터셋 ID |
| `--num-rows N` / `--page N` | 페이징 |

```bash
vworld catalog datasets --gid-cd 01 --num-rows 100
vworld catalog gids
vworld catalog gid-datasets --gid-cd 01
```

- **함정(실측)**: `gid-datasets`는 일부 분류(`--gid-cd 02`/`03`)에서 서버 응답에 이스케이프 안 된 제어문자가 포함되어 "JSON 파싱 실패" 에러 — `--raw`로 원응답을 받아 우회.

## staticmap — 정적 지도 이미지 (`/req/image`, GetMap)

```
vworld staticmap [OPTIONS] --zoom <ZOOM> <CENTER>
```

| 옵션 | 설명 |
|------|------|
| `CENTER` | 중심 좌표 `"x,y"` (필수) |
| `--zoom N` | 줌 레벨 6~18 (필수) |
| `--size w,h` | 이미지 크기 (기본 512,512 / 최대 1024,1024) |
| `--basemap <type>` | NONE / GRAPHIC(기본) / GRAPHIC_WHITE / SATELLITE / HYBRID |
| `--format <fmt>` | 이미지 포맷 (기본 png) |
| `--crs <CRS>` | 좌표계 (기본 EPSG:4326) |
| `-o, --output <path>` | 저장 경로 (기본 staticmap.png) |

```bash
vworld staticmap "127.0,37.5" --zoom 14 --size 512,512 -o map.png
```

## legend — 범례 이미지 / SLD (`/req/image`)

```
vworld legend [OPTIONS] <LAYER>
```

| 옵션 | 설명 |
|------|------|
| `LAYER` | 대상 레이어 (필수) |
| `--style <style>` | 스타일명 — **사실상 필수**(없으면 547B "결과없음"). 보통 layer와 동일(예: `lt_c_uq111`) |
| `--type ALL\|LAYER\|SUB` | 범례 타입 (기본 ALL) |
| `--sld` | SLD 스타일 정의(XML) 조회 — request=GetLegendStyle. 부울 플래그, 출력 경로는 `-o` |
| `--format <fmt>` | 기본 png |
| `-o, --output <path>` | 저장 경로 (기본 legend.png) |

```bash
vworld legend lt_c_uq111 --style lt_c_uq111 --type ALL -o legend.png
vworld legend lt_c_uq111 --style lt_c_uq111 --sld -o legend.sld.xml
```

## tile — 타일 (WMTS/TMS/벡터 통합)

```
vworld tile [OPTIONS] <SCHEME>       # SCHEME: wmts | tms | vector | vector-style | wmts-themes | wmts-capabilities
```

| 옵션 | 설명 |
|------|------|
| `--layer <layer>` | WMTS/TMS: Base(기본)/white/midnight/Hybrid/Satellite. 벡터 MVT: **poi/traffic만**(Base는 래스터 png/jpeg 전용, 실측) |
| `--z` / `--row` / `--col` | 줌 / 타일 행 / 타일 열. **wmts·tms는 row=Y, col=X — vector는 반대(row=X, col=Y)**(실측) |
| `--ext <ext>` | 확장자 — wmts/tms 기본 png, vector 기본 pbf(래스터 벡터: png/jpeg) |
| `--category` / `--year` / `--city` | **wmts-themes 전용** — 테마 카테고리(예: cities)/영상 연도/도시명 |
| `-o, --output <path>` | 저장 경로 |

```bash
# 서울시청 z14 = Y(row) 6449, X(col) 13969 (실측 검증 좌표)
vworld tile wmts --layer Base --z 14 --row 6449 --col 13969 -o tile.png
vworld tile tms  --layer Base --z 14 --row 6449 --col 13969 -o tile.png    # 같은 WMTS 좌표 입력, Y축 자동 반전
vworld tile vector --layer traffic --z 14 --row 13969 --col 6449 -o t.pbf  # MVT — row=X, col=Y (축 반대!)
vworld tile vector --layer poi --z 15 --row 27941 --col 12689 -o poi.pbf   # poi는 z≥15부터 데이터
vworld tile vector --layer Base --z 14 --row 13969 --col 6449 --ext png -o v.png  # 래스터 벡터
vworld tile vector-style --layer vectorStylePoi                            # 스타일 JS/JSON
vworld tile wmts-themes --category cities --year 2025 --city Oslo --z 11 --row 1086 --col 596 -o oslo.png
vworld tile wmts-capabilities -o WMTSCapabilities.xml                      # 능력문서(XML)
```

- **함정(실측)**: wmts/tms는 `--row`=Y·`--col`=X, **vector는 `--row`=X·`--col`=Y로 반대**. MVT `--layer Base`는 `InvalidParameterValue: tiletype [poi, traffic]` 에러. poi는 z14에서 범위 밖 — z≥15 사용. TMS는 입력 row(WMTS 기준)를 CLI가 자동 반전.

## map — 지도 임베드 HTML 생성

```
vworld map [OPTIONS] [KIND]
# KIND: 2d(기본) | 3d | 3dsim | ol | marker | chart | theme | text | controller | choropleth | 3d-extrude
```

CLI는 렌더링하지 않고 **HTML/URL/설정을 생성**한다. `-o` 미지정 시 JSON으로 URL·스니펫 보고. 모든 산출물에 토스 디자인 + 절대경로(https://) 적용, vworld 로고·안내문구 CSS 숨김.

> **2d vs ol 구분**: `map 2d` = 3D엔진(Cesium/WebGL) 평면 모드 / `map ol` = OpenLayers 2D 데이터레이어. 데이터(GeoJSON/마커/차트/주제도)를 얹으려면 **ol 계열**을 쓴다.

### 공통 옵션

| 옵션 | 설명 |
|------|------|
| `--center lon,lat` | 중심 좌표 (기본 127.0,37.5) |
| `--address "<주소>"` | 중심 주소(geocode 변환) — `--center`보다 우선 |
| `--zoom N` | 줌/높이 레벨 (기본 11) |
| `--basemap <type>` | GRAPHIC(기본) / GRAPHIC_WHITE / GRAPHIC_NIGHT / PHOTO / PHOTO_HYBRID (ol/marker/chart/theme) |
| `-o, --output <path>` | HTML 저장 |
| `--open` | 생성 HTML을 OS 기본 브라우저로 열기(`-o` 저장된 경우만) |
| `--no-search` | 주소 검색창 숨김(데이터 시각화 전용) |

- 입력 좌표는 EPSG:4326(lon,lat) 기본.

### map 2d / 3d — 기본 지도

```bash
vworld map 2d --center 127,37.5 -o map.html     # 3D엔진 평면 모드
vworld map 3d --center 127,37.5 -o globe.html   # 3D 지구본
```

### map 3dsim — 3D 분석·시뮬레이션 (49종)

```bash
vworld map 3dsim --analysis list                # 전체 목록(JSON, 49종)
vworld map 3dsim --analysis slope --address "남산공원길 105" -o slope.html
vworld map 3dsim --analysis sunlight --center 127.0,37.5 -o sunlight.html
```

| API | 종류 |
|-----|------|
| 1.0 분석 (11) | `slope` 경사도 · `terrainvolume` 토공량 · `profile` 지형단면 · `sunlight` 일조량 · `sunlightrights` 일조권 · `sunlightslope` 일조사선제한 · `visiblearea` 가시면적 · `viewsurface` 시곡면 · `culheritalter` 문화재현상변경 · `route` 드론·차량주행 · `buildingcontrol` 건물편집 |
| 2.0 가시화 (4) | `heatmap` · `cluster` · `grid` · `hexbin` |
| 3.0 샘플 (34) | `responsive` `lod4texture` `mapcontroller` `mapoption` `moveto` `geometry` `geometryz` `wms` `buildinginfo` `cameraturn` `flight` `rotateface` `rotateground` `driving` `markerevent` `circle` `regularshape` `specialshape` `imagesave` `geojson` `wfs` `glb` `wmswfs` `search` `dataapi` `wmts` `home` `measure` `buildingroll` `draw` `markergroup` `boundary` `editfeature` `popup` |

- 분석 결과값(경사도 분포·토공량 등)은 **브라우저(Cesium/WebGL)에서만 계산** — HTML을 브라우저로 열거나 Playwright로 자동 추출(상세: `3dsim_analysis_params.md`, README).
- 값 반환 대상: slope·terrainvolume·profile·sunlight·sunlightrights·visiblearea·viewsurface. route·buildingcontrol·heatmap·cluster·grid·hexbin은 반환값 없음(시각화).

### map ol — OpenLayers 2D + 벡터

| 옵션 | 설명 |
|------|------|
| `--polygon "lon,lat;…"` | 인라인 폴리곤(최소 3정점) |
| `--geojson <file>` | GeoJSON FeatureCollection(폴리곤/포인트/라인) — 자동 extent 맞춤 |
| `--kml <https URL>` | 외부 KML(절대경로 https만 — CORS) |
| `--route "lon,lat;…"` | 사전 경로 폴리라인(최소 2점) |
| `--zoom-control` / `--basemap-switch` / `--popup` | PanZoomBar / 배경 전환 버튼 / 클릭 좌표 팝업 |
| `--overview` / `--toolbar` | 미니맵 / 측정 툴바(거리·면적 등 11종) |

```bash
vworld map ol --center 127,37.5 --zoom 13 --basemap PHOTO --geojson f.geojson -o m.html
vworld map ol --polygon "127.0,37.5;127.01,37.5;127.01,37.51" --toolbar -o m.html
```

### map marker — 마커 + 팝업

```bash
vworld map marker --points markers.json -o m.html
```

- `markers.json`: `[{x, y, epsg?, title, contents, iconUrl?, text?, attr?}]` — epsg 기본 EPSG:4326(x=lon, y=lat).

### map chart — 위치 기반 차트

```bash
vworld map chart --type bar --data chart.json [--group] -o m.html    # bar | stackedbar | pie
```

- `chart.json`: `[{pos:[lon,lat], title, size?:[w,h], radius?(pie), styles:[{color,label,legendLabel?}], values:[…]}]` — stackedbar는 `values:[[…],[…]]` 중첩.
- `--group`: ChartGroup으로 묶어 표시.

### map theme — WMS 주제도 토글

```bash
vworld map theme --layers "도시지역:LT_C_UQ111,관리지역:LT_C_UQ112" -o m.html
```

### map text — 대량 포인트 클러스터링

```bash
vworld map text --file points.txt --epsg EPSG:4326 --distance 40 -o m.html
```

- `points.txt`: vworld TEXT 포맷 `lon⇥lat⇥title⇥desc⇥iconSize`(탭 구분, 헤더 1줄). **자기완결 상한 500줄**(초과 시 거부).
- `--distance`: 클러스터링 거리(기본 40).

### map controller — 2D/3D 전환 지도

```bash
vworld map controller --center 127,37.5 -o m.html    # vw.MapController
```

### map choropleth — 단계구분도(값별 색칠)

```
vworld map choropleth --geojson <joined.geojson> --value-field <수치필드> [옵션] -o out.html
```

| 옵션 | 설명 |
|------|------|
| `--value-field <prop>` | **필수** — 색칠 기준 properties 수치 키(문자열 숫자도 파싱) |
| `--color-scale <ramp>` | ylorrd(기본) / blues / greens / reds / viridis / rdylbu. `rdylbu`는 diverging(파랑↔빨강) 풀레인지, 나머지는 흰색·검정 극단 자동 회피 |
| `--classes N` | 구간 수 (기본 5) |
| `--class-method quantile\|equal` | 분류 방법 (기본 quantile) |
| `--breaks a,b,c,d` | 수동 경계값 — 지정 시 class-method 무시 |
| `--no-data-color <hex>` | 값 없는 feature 색 (기본 #cccccc) |
| `--opacity 0-1` | 채움 투명도 (기본 0.78) |
| `--legend` | 범례 패널 표시(제목 + 최저·최고 요약 + 색 스와치 + 구간 라벨, 천단위 콤마) |
| `--legend-title <text>` | 범례 제목(한글 가능, 기본 = value-field) |
| `--legend-pos <pos>` | top-right(기본) / top-left / bottom-right / bottom-left |

```bash
vworld map choropleth --geojson joined.geojson --value-field 인구 \
  --color-scale rdylbu --classes 5 --legend --legend-title "울산 인구(명)" \
  --legend-pos top-right --no-search --open -o map.html
```

- 인터랙션(자동): hover 강조, 폴리곤 클릭 시 값 토스트, 흰색 경계선.
- 색 구간은 Rust에서 계산(결정적), 순수 ol9 + VWorld 타일로 100% 렌더.

### map 3d-extrude — 폴리곤 3D 돌출 지도 (deck.gl)

GeoJSON 폴리곤을 수치값만큼 3D 높이로 세운다(extruded + VWorld WMTS 타일).

| 옵션 | 설명 |
|------|------|
| `--elevation-field <prop>` | **필수** — 높이로 쓸 properties 수치 키 |
| `--elevation-scale <S>` | "auto"(기본, 전체 범위를 max-height로 정규화) 또는 수치(h = v × scale) |
| `--max-height <M>` | auto 스케일 시 최대 높이(미터, 기본 4000) |
| `--pitch <P>` | 카메라 틸트 각도 (기본 50) |
| `--value-field <prop>` | 색칠 기준(미지정 시 elevation-field 재사용) |
| (색·범례) | `--color-scale` `--classes` `--class-method` `--breaks` `--no-data-color` `--opacity` `--legend` `--legend-title` `--legend-pos` — choropleth와 동일 |

```bash
vworld map 3d-extrude --geojson joined.geojson --elevation-field 인구 \
  --color-scale ylorrd --classes 5 --legend --legend-title "인구 3D" \
  --max-height 3000 -o out.html --open
```

### 인구·통계 choropleth 4단계 파이프라인 (kosis/sgis 연계)

```bash
# 1) 경계 GeoJSON (SGIS)                 2) 통계값 (KOSIS/SGIS, adm_cd 기준)
sgis boundary hadmarea --year 2022 --adm-cd 11 --low-search 1 --wgs84 -o sgg.geojson
kosis d <표ID> -c1 11 -f json -o pop.json

# 3) adm_cd 조인                          4) 색칠 지도
vworld data join --geojson sgg.geojson --table pop.json --on adm_cd \
  --table-key <키필드> --table-value <값필드> --as population -o joined.geojson
vworld map choropleth --geojson joined.geojson --value-field population \
  --color-scale ylorrd --classes 5 --legend --open -o pop_map.html
```

## batch — 다건 배치 실행

```
vworld batch [OPTIONS] --from <FILE> <COMMAND>     # COMMAND: 현재 geocode 지원
```

| 옵션 | 설명 |
|------|------|
| `--from <file>` | 입력 파일(줄당 1건) |
| `--type <TYPE>` | 기본 ROAD |
| `--reverse` | 역지오코딩 배치 |

```bash
vworld batch geocode --from addrs.txt --concurrency 4
```

> `geocode --input` / `ned --input`으로도 배치 가능 — batch는 geocode 전용 진입점.

## hjd-db — 행정동 경계 SQLite (오프라인)

`--by-hjd` 고속화용 선택적 DB. 없어도 `--by-hjd`는 역지오 폴백으로 동작한다.

```
vworld hjd-db build  --shp <BND_ADM_DONG_PG.shp> --db <hjd.sqlite>   # 경계 SHP→SQLite(폴리곤+bbox 인덱스)
vworld hjd-db region --xlsx <센서스지역코드.xlsx> --db <hjd.sqlite> [--sheet "2025년 6월"]
vworld hjd-db info   --db <hjd.sqlite>                               # 적재 행정동 수
vworld hjd-db lookup --db <hjd.sqlite> <동명|ADM_CD>                 # 경계+지역코드 조인 조회
```

| 서브커맨드 | 설명 |
|-----------|------|
| `build` | 행정동 경계 SHP(EPSG:5186)를 SQLite로 적재. 폴리곤 blob + bbox 인덱스(영역 질의 가속) |
| `region` | 센서스 지역코드 xlsx → `region_code` 테이블(시도/시군구/읍면동). `--sheet`로 연도 시트 선택(기본 "2025년 6월") |
| `info` | DB 요약(행정동 수) |
| `lookup` | 행정동코드(8자리) 또는 동명 일부로 조회 |

- **두 테이블**: `hjd`(경계 폴리곤·ADM_CD·ADM_NM), `region_code`(ADM_CD·명칭). 조인 키 = **ADM_CD(시도2+시군구3+읍면동3)**. 3,558/3,559(99.97%) 일치.
- 1회 적재 후 `ned --by-hjd --hjd-db`로 재사용(129MB SHP 재파싱 불필요). DB·SHP 분류 결과 동일(검증됨). 대용량 파일은 `.gitignore` 권장.

## config — 인증키·설정 관리

```
vworld config add-key <KEY> [--alias <이름>] [--referer <URL>]   # 키 추가(중복 거부)
vworld config list-keys                                           # 마스킹 목록
vworld config remove-key <KEY|인덱스>                             # 값 또는 인덱스로 제거
vworld config test-keys                                           # 실 호출 검증(유효/만료/도메인불일치 가이드)
vworld config path                                                # config.toml 실제 경로
```

- 설정 파일: `~/.vworld/config.toml` (Windows `%USERPROFILE%\.vworld\config.toml`). `--config`로 오버라이드.
- 등록된 모든 키는 동시성 키 풀에 자동 편입.

## update — 자가 업데이트 (GitHub Releases)

```
vworld update                    # 최신 버전으로 다운로드·교체 + 스킬 파일 갱신
vworld update --check            # 새 버전 확인만(교체 안 함)
vworld update --version v0.2.0   # 특정 버전 태그로 교체(버전 비교 생략 — 롤백 가능)
vworld update --yes              # 확인 프롬프트 생략(CI·비대화형)
vworld update --force            # 같은 버전이어도 다시 받아 교체(설치본 복구)
vworld update --skill-only       # 바이너리는 두고 스킬 파일만 갱신
vworld update --no-skill         # 스킬은 건너뛰고 바이너리만 교체
```

진행 순서 — 각 단계는 개별 확인 프롬프트를 거친다.

```
현재 버전: v0.2.1
최신 버전 확인 중...
최신 버전: v0.3.0
  체크섬 파일 다운로드 중...
  바이너리 다운로드 중 (vworld-macos)...
바이너리를 업데이트하겠습니까? (v0.2.1 → v0.3.0) (y/N) y
  SHA256 체크섬 검증 중...
  체크섬 검증 완료.
  바이너리 교체 중 (/Users/me/.local/bin/vworld)...
  바이너리: /Users/me/.local/bin/vworld
스킬 파일(SKILL.md, INSTALL.md, references 등)을 업데이트하겠습니까? (y/N) y
  스킬 파일 다운로드 중...
  스킬 체크섬 검증 중...
  스킬 체크섬 검증 완료.
  스킬: /Users/me/.claude/skills/vworld
  스킬: /Users/me/.codex/skills/vworld
vworld v0.2.1 → v0.3.0 업데이트 완료
```

| 항목 | 동작 |
|------|------|
| 체크섬 | 릴리스의 `SHA256SUMS`로 다운로드를 검증. 불일치면 즉시 중단. 체크섬 자산이 없는 구 릴리스(v0.2.1 이하)는 경고 후 진행 |
| 스킬 갱신 대상 | `~/.claude/skills/vworld`, `~/.codex/skills/vworld`, `$PWD/.claude/skills/vworld`, `$PWD/.codex/skills/vworld` 중 **이미 존재하는 것만**. 없으면 건너뜀(새로 만들지 않음) |
| 스킬 자산 | `vworld-skill-files.zip`(문서·레퍼런스만, 바이너리 제외 — 업데이트 트래픽 절감). 설치용 `vworld-skill.zip`은 바이너리 포함으로 그대로 유지 |
| 스킬 내 바이너리 | 스킬 디렉터리에 `app/vworld` 사본이 있으면 함께 교체(stale 방지). 없으면 아무것도 하지 않음 |
| 비대화형 | stdin이 터미널이 아니면 프롬프트는 자동 "아니오" — CI에서 의도치 않은 교체가 없다. `--yes`로 전부 승인 |
| 실행 중 교체 | 임시파일을 **대상과 같은 디렉터리**에 만들고 rename(EXDEV 회피). Windows는 기존 exe를 `.old`로 선이동 |
| 권한 부족 | `/usr/local/bin` 등은 rename이 거부될 수 있음 — sudo 또는 install.sh 재실행 안내 |

- 평소 실행 시 하루 1회 자동 감지·알림(stderr, 다운로드 없음). 끄기: `export VWORLD_NO_UPDATE_CHECK=1` (CI는 자동 생략).

---

## 함정 모음 (요약 — 상세는 LEARNINGS.md)

| 함정 | 대처 |
|------|------|
| 주소 유형 불일치(ROAD↔PARCEL)면 빈 결과 | `geocode --type auto`(기본) 사용 |
| `search --type ADDRESS/DISTRICT`는 category 필수 | ADDRESS=`--category ROAD\|PARCEL`, DISTRICT=`--category L1~L4` |
| `data --geom-filter`에 EPSG 접미사 → INVALID_RANGE | `BOX(minx,miny,maxx,maxy)` 좌표 4개만(4326 해석) |
| `data --geom-filter` BOX/POLYGON 면적 10km² 초과 → INVALID_RANGE | 분할 조회 또는 `ned <WFS> --pnu --all` |
| `data --emd-cd` 단독 → INVALID_RANGE(일부 데이터셋) | geom/attr 필터 사용. 동 전수는 `ned <WFS> --pnu <8자리> --all` |
| WMS/WFS `EPSG:4326`·`5185~5188` bbox는 `(ymin,xmin,ymax,xmax)` 위경도 반전 | 위도 먼저 |
| NED WMS 계열은 이미지 | 데이터는 WFS/속성(data) 계열 |
| wmts/tms `--row`=Y·`--col`=X, **vector는 반대**(`--row`=X·`--col`=Y) | TMS는 CLI가 Y축 자동 반전 |
| MVT `--layer Base` 불가 / poi z14 범위 밖 | MVT는 poi(z≥15)·traffic만. Base는 `--ext png` 래스터 |
| `legend`는 `--style` 없으면 547B "결과없음" | style은 보통 layer명과 동일 |
| `catalog gid-datasets` 일부 gid-cd JSON 파싱 실패(서버 제어문자) | `--raw`로 우회 |
| HTTP 200 본문 에러 | CLI가 자동 검사 — "결과 없음"은 정상(빈 결과) |
| `--hjd-db`는 자동 참조 안 됨 | 경로를 반드시 명시 |
| VWorld WFS maxFeatures 1000 cap, startIndex 미지원 | `ned --all`이 PNU 접두 분할로 자동 우회 |
