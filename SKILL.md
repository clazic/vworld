---
name: vworld
description: VWorld OpenAPI(지오코딩·검색·2D데이터·국가중점데이터·WMS/WFS·타일·StaticMap·지도임베드)를 호출하는 자기완결 Rust CLI. "이 주소 좌표 알려줘", "용도지역 조회", "이 좌표 주소 뭐야", "지적도/공시지가/건물정보 같은 공간데이터", "지도 이미지 저장" 류 한국 공간정보 질의에 사용.
---

# VWorld CLI 스킬

VWorld OpenAPI를 도구로 사용하는 스킬. 바이너리는 `app/vworld`(자기완결), 인증키는 **사용자 홈의 `~/.vworld/config.toml`**(Windows `%USERPROFILE%\.vworld\config.toml`)에 저장된다 — 바이너리 옆에 설정파일을 둘 필요 없고, 다른 파일을 쓰려면 `--config <경로>`로 지정한다.

## 사용 전 필수 (자기학습 루프)

1. **작업 전** `LEARNINGS.md`를 먼저 읽어라(과거 함정·성공 쿼리).
2. **작업 후** 새로 배운 실패·함정·성공 쿼리를 `LEARNINGS.md`에 5요소 형식으로 1줄+ append하라.

## 바이너리 호출

```
app/vworld [전역옵션] <명령> [인자]
```

전역옵션: `--pretty`(들여쓰기) `--raw`(원응답) `--concurrency N` `--timing` `--referer <URL>` `--config <path>`.
출력은 **JSON 기본**(stdout). 이미지형은 파일 저장 + 경로 JSON 보고.

명령 전수(16종): geocode(`geo`) · geocoder · search(`s`) · data · ned · wms · wfs · catalog · staticmap(`static`) · legend · tile · map · batch · hjd-db · config · update.
**전 명령·전 옵션 상세와 실측 검증된 예시는 `references/docs/USAGE.md`** (2026-07-16, 76케이스 실사용 테스트 통과본).
CLI 자체에도 **파라미터별 상세 설명**이 들어 있다 — `vworld <명령> --help`는 값 형식·허용값·기본값·함정·예시까지, `-h`는 한 줄 요약만 보여준다. 옵션 의미가 불확실하면 USAGE.md 또는 `--help`로 확인한 뒤 호출한다.

## 자연어 의도 → 명령 매핑

| 사용자 의도 | 명령 |
|-------------|------|
| "이 주소 좌표 알려줘" | `vworld geocode "<주소>"` (`--type auto` 기본 — 도로명→지번 자동 폴백) |
| "이 좌표 주소 뭐야" | `vworld geocode "<x,y>"` (좌표 입력 시 자동 역지오. 수동이면 `--reverse --type BOTH`) |
| "장소/건물 검색" | `vworld search "<키워드>" --type PLACE` |
| "행정구역 검색" | `vworld search "<키워드>" --type DISTRICT --category L2` (**category 필수** — L1 시도/L2 시군구/L3 읍면동/L4 리) |
| "지번 주소 검색" | `vworld search "<주소>" --type ADDRESS --category PARCEL` (**category 필수** — ROAD\|PARCEL) |
| "도로명 검색" | `vworld search "<키워드>" --type ROAD` |
| "이 영역 지적도/공간데이터" | `vworld data <데이터셋ID> --geom-filter "BOX(minx,miny,maxx,maxy)"` (**EPSG 접미사 금지 + 면적 10km² 이내** — 넘으면 INVALID_RANGE. 예: `LP_PA_CBND_BUBUN`) |
| "이 주소 필지의 용도지역/공시지가/건물속성" | **표준 3단계 체인**: `geocode`(좌표) → `data LP_PA_CBND_BUBUN --geom-filter "POINT(x y)"`(properties.pnu) → `ned <op> --pnu <PNU>` |
| "건축물연령/공시지가/용도지역 등 국가중점데이터" | `vworld ned <오퍼레이션> --pnu <필지번호>` (목록: `vworld ned --list`, 변수: `--params`) |
| "OO동 **전체 필지**의 공시지가/속성" | `vworld ned <WFS오퍼레이션> --pnu <법정동8자리> --all` (1000 cap 자동 우회) |
| "OO동을 **행정동별로** 공시지가 비교" | `vworld ned getIndvdLandPriceWFS --pnu <법정동8자리> --by-hjd` (역지오 행정동 분류+통계, 자동 재처리) |
| "여러 PNU 한꺼번에" | `vworld ned <data오퍼레이션> --input pnus.txt --concurrency 6` |
| "여러 주소 한꺼번에" | `vworld geocode --input addrs.txt --concurrency 4` (또는 `vworld batch geocode --from addrs.txt`) |
| "WMS 레이어 능력/맵" | `vworld wms --request GetCapabilities` / `--request GetMap --layers ... --bbox ...` |
| "WFS 지리객체" | `vworld wfs --request GetFeature --typename ... --bbox ...` (`-o v.html`이면 토스 디자인 HTML 뷰어) |
| "다운로드 카탈로그" | `vworld catalog datasets --gid-cd 01` (gid-datasets는 일부 gid-cd에서 서버 제어문자로 파싱 실패 → `--raw` 우회) |
| "지도 이미지 저장" | `vworld staticmap "<x,y>" --zoom 14 --size 512,512 -o map.png` |
| "VWorld 레이어 범례 이미지" | `vworld legend <레이어> --style <레이어명> --type ALL -o legend.png` (**`--style` 필수** — 없으면 547B "결과없음". style은 보통 layer와 동일. 예: `lt_c_uq111`) |
| "범례 SLD 스타일" | `vworld legend <레이어> --style <레이어명> --sld -o legend.sld.xml` (GetLegendStyle, `--sld`는 부울 플래그·출력 경로는 `-o`) |
| "배경지도 타일" | `vworld tile wmts --layer Base --z 14 --row <Y> --col <X> -o tile.png` (서울 z14 실측: row 6449, col 13969) |
| "벡터 타일(MVT)" | `vworld tile vector --layer traffic --z 14 --row <X> --col <Y> -o t.pbf` (**축이 wmts와 반대**. MVT는 poi(z≥15)·traffic만, Base는 `--ext png` 래스터 전용) |
| "WMTS 주제도 목록(해외위성 등 시계열)" | `vworld tile wmts-themes --category cities --year 2025 --city Oslo --z 11 --row 1086 --col 596 -o oslo.png` |
| "WMTS 능력문서(메타데이터)" | `vworld tile wmts-capabilities -o WMTSCapabilities.xml` |
| "지도 띄우는 HTML" | `vworld map 2d --center 127,37.5 -o map.html` (3d/3dsim 가능) |
| "2D 데이터지도(벡터/마커/차트/주제도)" | `vworld map ol\|marker\|chart\|theme\|text ...` (OpenLayers 2D — 아래 섹션) |
| "GeoJSON/폴리곤 지도에 표시" | `vworld map ol --geojson f.geojson -o m.html` / `--polygon "lon,lat;…"` |
| "인구/통계 데이터를 지도에 **색칠**(단계구분도/choropleth)" | `vworld map choropleth --geojson f.geojson --value-field <수치필드> --color-scale ylorrd --classes 5 --legend --open` |
| "경계 GeoJSON에 통계표(인구 등) 값 **조인**" | `vworld data join --geojson 경계.geojson --table 통계.json --on adm_cd --table-key adm_cd --table-value <값필드> --as <속성명> -o joined.geojson` |
| "**kosis/sgis 인구를 vworld 2d지도에 표시**" | 4단계 파이프라인 — 아래 `## 인구·통계 choropleth 워크플로` 참조 |
| "2D 데이터레이어 158종 탐색" | `vworld data layers` (전체 목록; `--search <키워드>` / `--cat <카테고리>` / `--geom <타입>` 필터) |
| "특정 2D 레이어 속성 확인" | `vworld data describe <데이터ID>` (속성표·단일검색키·샘플URL) |
| "연속지적도 DXF 내보내기" | `vworld ned getCtnlgsSpceWFS --address "<주소>" --radius 1000 --dxf parcels.dxf` (기본 EPSG:5187, `--dxf <경로>` 는 경로 인자) |
| "연속지적도 SHP 내보내기" | `vworld ned getCtnlgsSpceWFS --address "<주소>" --radius 1000 --shp parcels.shp` (속성포함 5종 생성, `--shp <경로>` 는 경로 인자) |
| "CLI 업데이트" | `vworld update` — 바이너리 교체 + 스킬 파일 갱신 (`--check` 확인만 / `--yes` 비대화형 / `--version vX.Y.Z` 롤백 / `--skill-only` / `--no-skill`) |

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

## 자가 업데이트 (update)

`vworld update` 한 번으로 **바이너리 교체 + 이 스킬 파일 갱신**이 모두 이뤄진다. 단계마다 y/N 확인을 거치고, 릴리스의 `SHA256SUMS`로 다운로드를 검증한다.

```
vworld update                  # 바이너리 + 스킬 파일 (각각 확인)
vworld update --check          # 새 버전 유무만 확인
vworld update --yes            # 확인 생략(비대화형)
vworld update --skill-only     # 스킬 문서만 갱신
vworld update --no-skill       # 바이너리만 교체
vworld update --version vX.Y.Z # 특정 버전으로 롤백(버전 비교 생략)
vworld update --force          # 같은 버전 재설치(설치본 복구)
```

- 갱신 대상 스킬 경로: `~/.claude/skills/vworld`, `~/.codex/skills/vworld`, `$PWD/.claude|.codex/skills/vworld` 중 **이미 존재하는 것만**. 없으면 건너뛴다(새로 만들지 않음).
- 스킬 안에 `app/vworld` 사본이 있으면 **함께 교체**되어 버전이 어긋나지 않는다.
- 체크섬 불일치는 즉시 중단. 체크섬 자산이 없는 구 릴리스(v0.2.1 이하)는 경고 후 진행.
- 비대화형(파이프·CI)에서는 프롬프트가 자동 "아니오" — 의도치 않은 교체가 없다. 자동 알림 끄기: `VWORLD_NO_UPDATE_CHECK=1`.
- 갱신되는 파일: `SKILL.md` · `INSTALL.md` · `README.md` · `references/`. `LEARNINGS.md`와 `scripts/`는 덮어쓰지 않는다(자기학습 기록 보존).

## 레퍼런스 문서 (references/docs/)

- `references/docs/USAGE.md` — **전 명령·전 옵션 사용설명서**(76케이스 실사용 테스트 검증, 함정 표 포함). 명령 사용법이 불확실하면 이 문서 우선.
- `references/docs/rest_api_catalog.md` — 13종 REST 엔드포인트·파라미터·옵션 전수.
- `references/docs/national_data_catalog.md` — 국가중점데이터(NED) 115 오퍼레이션 전수.

## 함정 (요약 — 상세는 LEARNINGS.md·USAGE.md 함정 표)

- **주소 유형**: 도로명=ROAD, 지번=PARCEL. 틀리면 결과 없음(빈 result). 모르면 `--type auto`(기본).
- **search ADDRESS/DISTRICT는 `--category` 필수**: ADDRESS=ROAD|PARCEL, DISTRICT=L1~L4. 없으면 PARAM_REQUIRED 에러.
- **data geomFilter에 EPSG 접미사 금지**: `BOX(...,EPSG:4326)` → INVALID_RANGE. 좌표 4개만(4326 해석).
- **data geomFilter BOX/POLYGON 면적 10km² 이내**(서버 제한): 넘으면 INVALID_RANGE("요청면적이 10km² 이내"). 넓은 영역은 분할 조회 또는 `ned <WFS> --pnu --all` 사용.
- **data --emd-cd 단독 거부**: 일부 데이터셋(연속지적도 등)에서 INVALID_RANGE — geom/attr 필터 사용. 동 전수는 `ned <WFS> --pnu <8자리> --all`.
- **bbox 축순서**(공식 가이드): WMS는 `EPSG:4326·5185·5186·5187·5188`일 때 `(ymin,xmin,ymax,xmax)` 위경도 반전. WFS는 `EPSG:4326`일 때만 반전, 그 외 `(xmin,ymin,xmax,ymax)`.
- **WFS 기본 좌표계는 EPSG:900913**(Google Mercator) — srsname 미지정 시 좌표가 1.4e7대로 나오면 이것. lon/lat 원하면 `--crs EPSG:4326`(NED WFS는 `--param srsName=EPSG:4326`).
- **NED WMS 계열**은 이미지(타일/staticmap 경로). 데이터형은 WFS/속성(data) 계열.
- **타일 좌표**: wmts/tms `--row`=Y, `--col`=X — **vector는 반대**(`--row`=X, `--col`=Y). TMS는 CLI가 Y축 반전 자동 처리.
- **벡터 MVT 레이어**: poi(z≥15)·traffic만. `--layer Base`는 `--ext png/jpeg` 래스터 전용.
- **catalog gid-datasets**: gid-cd 02/03은 서버 응답 제어문자로 JSON 파싱 실패 — `--raw` 우회.
- **HTTP 200 본문 에러**: CLI가 자동 검사. "결과 없음"은 정상(빈 결과)로 처리됨.

## WMS/WFS 지원 좌표계 (공식 가이드 v4dv_wmsguide2)

| 좌표계 | EPSG 코드 |
|--------|-----------|
| WGS84 경위도 | **EPSG:4326** (WMS crs 기본값) |
| GRS80 경위도 | EPSG:4019 |
| Google Mercator | EPSG:3857, **EPSG:900913** (WFS srsname 기본값) |
| 서부원점(GRS80) | EPSG:5180(50만), EPSG:5185 |
| 중부원점(GRS80) | EPSG:5181(50만), EPSG:5186 |
| 제주원점(GRS80, 55만) | EPSG:5182 |
| 동부원점(GRS80) | EPSG:5183(50만), EPSG:5187 |
| 동해(울릉)원점(GRS80) | EPSG:5184(50만), EPSG:5188 |
| UTM-K(GRS80) | EPSG:5179 |

- bbox 축순서: WMS는 4326·5185~5188에서 `(ymin,xmin,ymax,xmax)` 반전, WFS는 4326에서만 반전.
- 출처: https://www.vworld.kr/dev/v4dv_wmsguide2_s001.do (WMS/WFS 공통정보 · 지원좌표계)

### CLI 명령별 기본 좌표계 (`--crs` 미지정 시)

| 명령 | 기본값 | 비고 |
|------|--------|------|
| `geocode` `geocoder` `search` `data` `wms` `wfs` `staticmap` | **EPSG:4326** | CLI가 명시 전송(서버 WFS 기본 900913을 덮어씀) |
| `ned` | **EPSG:5187**(동부원점 TM, 미터) | WMS/WFS 단건·DXF/SHP 내보내기. 중부 5186 / 서부 5185 / 위경도 4326은 `--crs`로 변경 |
| `ned --by-hjd` | EPSG:4326 내부 고정 | `--crs` 무시(경고 출력) |
| `map` 계열 입력(`--center`/`--polygon`/`--route`, text `--epsg`) | EPSG:4326 (lon,lat) | JS측 `ol.proj`가 내부 변환 |
| NED WFS 원응답(`--raw`·`--param` 직접 호출 시) | EPSG:900913 | srsName 미지정 시 서버 기본 — `--param srsName=EPSG:4326`으로 변경 |

## 통합 지오코더 (geocoder) · 자동 지오코딩 (geocode)

- `vworld geocoder "<주소 또는 x,y>"` → **좌표·지번·도로명을 한 번에**(apis.vworld.kr). 입력 형식 자동 감지(주소↔좌표).
- `vworld geocode "<주소>"` — `--type auto`가 기본(도로명→지번 자동 폴백). 좌표("x,y") 입력 시 자동 역지오코딩(`--reverse` 불필요). `--type ROAD|PARCEL` 수동 지정도 가능.

## 3D 분석·시뮬레이션 (map 3dsim --analysis) — 49종

`vworld map 3dsim --analysis <type> --address "<주소>"`(또는 `--center lon,lat`) `-o out.html`
- 목록: `vworld map 3dsim --analysis list` (**49종 실측** — API 1.0 분석 11 + 2.0 가시화 4 + 3.0 샘플 34)
- **1.0 분석(11)**: slope 경사도 · terrainvolume 토공량 · profile 지형단면 · sunlight 일조량 · sunlightrights 일조권 · sunlightslope 일조사선제한 · visiblearea 가시면적 · viewsurface 시곡면 · culheritalter 문화재현상변경 · route 드론·차량주행 · buildingcontrol 건물편집
- **2.0 가시화(4)**: heatmap · cluster · grid · hexbin
- **3.0 샘플(34)**: flight 비행 · driving 운전 · measure 측정 · draw 그리기 · buildinginfo 건물클릭 · geojson/wfs/wmts 레이어 · popup · markergroup 등 (전체는 `--analysis list`)
- 파라미터(위치·옵션·지도 인터랙션) 명세: `references/docs/3dsim_analysis_params.md`
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
> `vworld map --help`의 KIND 항목에 **11종 전부와 각 용도**가 정렬 출력된다(2d/3d/3dsim/ol/marker/chart/theme/text/controller/choropleth/3d-extrude).

| 명령 | 용도 | 예시 |
|------|------|------|
| `map ol` | 기본 2D 지도 + 벡터(폴리곤/GeoJSON) + 컨트롤 | `vworld map ol --center 127,37.5 --zoom 13 --basemap PHOTO --geojson f.geojson -o m.html` |
| `map marker` | 마커 + 팝업 | `vworld map marker --points markers.json -o m.html` |
| `map chart` | 위치 기반 차트(막대/누적/파이) + 범례 | `vworld map chart --type bar --data chart.json [--group] -o m.html` |
| `map theme` | WMS 주제도(named layer) + 토글 | `vworld map theme --layers "도시지역:LT_C_UQ111,관리지역:LT_C_UQ112" -o m.html` |
| `map text` | 대량 포인트(TEXTLayer 클러스터링) | `vworld map text --file points.txt --epsg EPSG:4326 --distance 40 -o m.html` |
| `map controller` | 2D/3D 전환 지도(vw.MapController) | `vworld map controller --center 127,37.5 -o m.html` |
| `map choropleth` | **값별 색칠 단계구분도**(순수 ol9, feature별 styleFunction) + 범례 | `vworld map choropleth --geojson joined.geojson --value-field population --color-scale ylorrd --classes 5 --legend --open` |
| `map 3d-extrude` | **GeoJSON 폴리곤을 수치값만큼 3D 높이로 세우는 deck.gl 지도** — extruded:true + VWorld WMTS 타일 | `vworld map 3d-extrude --geojson joined.geojson --elevation-field 인구 --value-field 인구 --color-scale ylorrd --classes 5 --legend --legend-title "인구 3D" --max-height 3000 -o out.html --open` |

### map 3d-extrude 옵션
- `--elevation-field <PROP>` (필수): 높이로 쓸 properties 수치 키.
- `--elevation-scale <S>` (기본 "auto"): "auto"면 데이터 전체 범위를 `--max-height`로 정규화, 수치면 `h = v * scale`.
- `--max-height <M>` (기본 4000.0): auto 스케일 시 최대 높이(맵 단위, 미터 기준).
- `--pitch <P>` (기본 50.0): 카메라 틸트 각도.
- `--value-field`: 색칠 기준 필드 (미지정 시 `--elevation-field` 재사용).
- `--color-scale`, `--classes`, `--class-method`, `--breaks`, `--no-data-color`, `--opacity`, `--legend`, `--legend-title`, `--legend-pos`, `--center`, `--zoom`, `--open`, `-o`: choropleth와 동일.

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
- 기존 WFS/GeoJSON 뷰어(`wfs ... -o viewer.html`)와 ol3 `map ol --geojson`은 **공존**(둘 다 GeoJSON 표시 가능).
- vworld 로고·"연속지적도…참고용" 안내문구는 모든 생성 HTML에서 CSS로 숨김 처리(`.vw-logo`/`.vw-notice`).
- 계획·검증 상세 문서는 저장소에서 삭제됐다(git 이력에만 존재). 실사용 함정은 `LEARNINGS.md`, 명령별 검증 예시는 `references/docs/USAGE.md`를 본다.

## 인구·통계 choropleth 워크플로 (kosis/sgis → vworld 2D지도)

"인구/통계 데이터를 vworld 지도에 색칠해줘" 류 요청은 **외부 코드(python/js) 작성 없이 CLI 4단계**로 끝낸다. 핵심 조인키는 **통계청 행정구역코드 `adm_cd`**(KOSIS C1 = SGIS adm_cd, 동일 채번이라 무가공 조인).

```bash
# 1) 경계 GeoJSON (행정동/시군구, WGS84, properties.adm_cd) — SGIS
sgis boundary hadmarea --year 2022 --adm-cd 11 --low-search 1 --wgs84 -o sgg.geojson
#    (year는 데이터 있는 연도로; 시군구=시도2자리+low_search, 행정동=시군구5자리+low_search)

# 2) 통계값 (adm_cd 기준) — KOSIS 또는 SGIS
kosis d <인구표ID> -c1 11 -f json -o pop.json          # KOSIS: C1=adm_cd
#   또는 sgis data population --year 2022 --adm-cd 11 --low-search 1 -f json -o pop.json  # SGIS: tot_ppltn

# 3) adm_cd로 통계값을 경계 properties에 조인
vworld data join --geojson sgg.geojson --table pop.json \
  --on adm_cd --table-key <통계측키필드> --table-value <값필드> --as population -o joined.geojson
#   매칭/미매칭 카운트를 JSON으로 보고(unmatched>0이면 키·연도 점검)

# 4) 색칠 지도 생성 + 자동 열기
vworld map choropleth --geojson joined.geojson --value-field population \
  --color-scale ylorrd --classes 5 --class-method quantile --legend --open -o pop_map.html
```

### map choropleth 옵션
- `--value-field <prop>`(필수): 색칠 기준 properties 수치 키(문자열 숫자도 파싱).
- `--color-scale ylorrd|blues|greens|reds|viridis|rdylbu`(기본 ylorrd) / `--classes N`(기본 5).
  - `rdylbu`는 **diverging**(파랑↔빨강) 팔레트로 양끝 풀레인지, 나머지(sequential)는 거의 흰색·검정 극단을 자동 회피.
- `--class-method quantile|equal`(기본 quantile) / `--breaks a,b,c,d`(수동 경계, 주면 method 무시).
- `--no-data-color <hex>`(기본 #cccccc) / `--opacity 0-1`(기본 0.78).
- `--legend`(범례 표시) / `--legend-title <text>`(범례 제목, **한글 가능**, 미지정 시 --value-field 값) / `--legend-pos top-right|top-left|bottom-right|bottom-left`(기본 top-right).
  - 범례 = 패널형(제목 + "최저 N · 최고 M" 요약 + 인라인 색 스와치 + 구간 라벨, 천단위 콤마).
- `--no-search`(주소 검색창 숨김 — 데이터 시각화 전용) / `--open`(생성 HTML을 OS 기본 브라우저로).
- 인터랙션(자동): **hover 강조**(테두리 진해짐+위로), 폴리곤 **클릭 시 값 토스트**(천단위 콤마), 흰색 경계선.
- 색 구간·램프는 Rust에서 계산(결정적·테스트됨), JS는 룩업만 — 순수 ol9 + VWorld 타일이라 feature별 색이 100% 렌더(vw.ol3.Map의 setStyle 누락 이슈 회피).

```bash
# 예: 울산 구·군 인구 — diverging 색 + 한글 범례 우상단 + 검색창 숨김 + 자동 열기
vworld map choropleth --geojson joined.geojson --value-field 인구 \
  --color-scale rdylbu --classes 5 --legend --legend-title "울산 인구(명)" \
  --legend-pos top-right --no-search --open -o map.html
```

### data join 옵션
- `--geojson`(경계) `--table`(통계 JSON 배열) `--on`(경계측 키, 기본 adm_cd) `--table-key`(통계측 키) `--table-value`(가져올 값) `--as`(주입 properties명) `-o`(출력).
- `--name-tail`: **이름 조인 폴백**. `--on adm_nm`처럼 이름으로 조인할 때 경계측 값의 마지막 공백토큰만 비교(예: "서울특별시 종로구 사직동" ↔ 통계 "사직동"). 코드가 안 맞는 통계표용. **같은 시군구 범위에서만 안전**(동명 행정동 주의).
- 미매칭 feature는 no-data(범례에서 회색) 처리. adm_cd 자릿수(시도2/시군구5/행정동8)가 양측 일치해야 함.

> **코드 일치 우선**: KOSIS C1 ↔ SGIS adm_cd는 **통계청 계열이면 시군구·행정동 모두 무가공 일치**(검증). 단 KOSIS 지자체 주민등록 읍면동표는 코드가 다를 수 있어 `--with-code`로 확인 필수 — 상세는 **`kosis` 스킬 `references/16-geo-join.md` §16.7**.
> 검증: 서울 25개 시군구 25/25, 종로구 17개 행정동 17/17 매칭 → choropleth + 범례 렌더 확인.

## 데이터 자원 (references/data)

### 빌드타임 codegen 입력 카탈로그 (런타임 파일 불요)

| 파일 | 설명 |
|------|------|
| `references/data/ned_catalog.tsv` | NED 오퍼레이션 카탈로그 (build.rs codegen 입력) |
| `references/data/ned_params.tsv` | NED 파라미터 정의 (build.rs codegen 입력) |
| `references/data/twod_catalog.tsv` | 2D 데이터레이어 158종 카탈로그 (build.rs codegen 입력) |
| `references/data/twod_attrs.tsv` | 2D 레이어 속성 정의 (build.rs codegen 입력) |
| `references/data/twod_seed.tsv` | 2D 레이어 시드 데이터 (참고용 — 빌드·런타임 미사용) |

상위 4종 TSV는 `build.rs`가 **빌드타임에 읽어 `OUT_DIR`에 Rust 정적 테이블 코드를 생성**하고, 그 산출물이 바이너리에 임베드된다. 런타임에 파일을 참조하지 않으므로 사용자가 별도로 신경 쓸 필요 없다. 코어(바이너리 + codegen 테이블)는 **자기완결**이다. `twod_seed.tsv`는 빌드·런타임 어디서도 읽지 않는 참고용 시드 데이터다.

### vworld.sqlite (132MB) — opt-in 런타임 자원

`vworld.sqlite`는 `--by-hjd` 행정동별 고속 처리를 위한 **선택적(opt-in) 런타임 DB**다. 코어 자기완결 범위 밖의 자원이며 `skills/app`에 동봉하지 않고 `references/data`에 유지한다.

- **디폴트 경로 없음** — `--hjd-db` 없이도 `--by-hjd`는 역지오코딩 폴백으로 정상 동작(sqlite 불요).
- **고속화 원하면 부트스트랩 1회 필요**:

```bash
# 1) 행정동 경계 SHP로 DB 생성
vworld hjd-db build --shp <행정동경계.shp> --db references/data/vworld.sqlite

# 2) --hjd-db 경로 인자로 명시 참조
vworld ned getIndvdLandPriceWFS --pnu <법정동8자리> --by-hjd --hjd-db references/data/vworld.sqlite
```

- `--hjd-db <path>`는 **경로 인자** — 자동 참조 안 됨, 반드시 명시.
- sqlite 미사용 시: `--by-hjd`가 역지오코딩 폴백으로 동작(느리지만 정상).
- 실측(2026-07-16): 신정동 31140104 → 11,831필지 전수, DB point-in-polygon 분류 커버리지 100%, 행정동별 `{count, hjd, stats:{mean,median,q1,q3,min,max}}` 반환.
- 부가 서브커맨드: `hjd-db info --db`(적재 수) / `hjd-db lookup --db <동명|ADM_CD>`(경계+지역코드 조인 조회) / `hjd-db region --xlsx --db`(센서스 지역코드 적재).

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

# 3단계: 실제 조회 — GetFeature 호출 (BOX 면적 10km² 이내!)
vworld data LP_PA_CBND_BUBUN --geom-filter "BOX(126.97,37.55,127.0,37.58)"
vworld data LP_PA_CBND_BUBUN --attr-filter "pnu:=:1114010300100310000"
```

> **팁**: `data layers` 출력의 데이터ID를 `data describe <id>`에 그대로 넣으면 단일검색키·속성표·샘플 URL을 한 번에 확인할 수 있다.
