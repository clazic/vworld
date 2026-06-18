# VWorld CLI — 명령별 사용법

전역 옵션: `--pretty --raw --concurrency N --timing --referer <URL> --config <path>`.
모든 데이터형은 JSON(stdout), 이미지형은 파일 저장(+경로 JSON 보고).

## geocode — 지오/역지오 (`/req/address`)
```
vworld geocode "서울특별시 중구 세종대로 110" --type ROAD
vworld geocode "127.0,37.5" --reverse --type BOTH      # 역지오(좌표→주소)
vworld geocode --input addrs.txt --concurrency 4       # 배치(줄당 1건, # 주석/빈줄 스킵)
```
- `--type`: ROAD(도로명) | PARCEL(지번) | BOTH(역지오). 함정: 유형 불일치 시 빈 결과.
- 출력: `response.result.point.{x,y}` + `refined` 정제주소.

## search — 검색 (`/req/search`)
```
vworld search "공간정보산업진흥원" --type PLACE
vworld search "판교로 344" --type ROAD --size 20 --page 1
```
- `--type`: PLACE | ADDRESS | DISTRICT | ROAD. `--category`, `--bbox minx,miny,maxx,maxy`, `--size`(1~1000)/`--page`.

## data — 2D데이터 (`/req/data`, 158 데이터셋)
```
vworld data LP_PA_CBND_BUBUN --geom-filter "BOX(127.0,37.5,127.1,37.6,EPSG:4326)"
vworld data <ID> --attr-filter "속성:=:값" --columns "col1,col2" --size 100
vworld data <ID> --emd-cd 4117310300        # geom/attr 둘 다 없을 때 필수
```
- `request=GetFeature`. 데이터셋 ID 카탈로그는 별도 harvest(후속).

## ned — 국가중점데이터 (`/ned/{wms|wfs|data}`, 115 오퍼레이션)
```
vworld ned --list                            # 115 오퍼레이션 목록(레지스트리)
vworld ned getBuildingAge --pnu 1111018300101970001
vworld ned getLandCharacteristics --pnu <PNU>

# WFS 전수 수집 — 법정동 단위 (1000건 cap 자동 우회: PNU 접두 적응분할)
vworld ned getIndvdLandPriceWFS --pnu 31140104 --all   # 신정동 전체 11,831필지 공시지가

# 행정동별 집계 — 법정동 전수 → 행정동 분류 → 통계 (단일 명령)
vworld ned getIndvdLandPriceWFS --pnu 26500101 --by-hjd          # 역지오 분류(인터넷 필요)
# ★ 최선: 행정동경계 기반 point-in-polygon (역지오 0회·즉시·100%·정확·오프라인)
vworld hjd-db build --shp BND_ADM_DONG_PG.shp --db hjd.sqlite   # 1회 적재(3,559 행정동)
vworld ned getIndvdLandPriceWFS --pnu 26500101 --by-hjd --hjd-db hjd.sqlite   # DB 기반(권장·빠름)
vworld ned getIndvdLandPriceWFS --pnu 26500101 --by-hjd --hjd-shp BND_ADM_DONG_PG.shp  # SHP 직접
vworld ned <WFS오퍼레이션> --pnu <법정동8자리> --by-hjd --value-field <필드>  # 다른 수치필드

# PNU 목록 병렬 배치 (data 속성 계열)
vworld ned getIndvdLandPriceAttr --input pnus.txt --concurrency 6
```
- WFS/속성(data) 계열만 데이터형. WMS 계열은 이미지(타일 경로 사용).
- 미수집 옵션은 `--param k=v`(반복)로 패스스루(key/domain은 거부).
- **`--all`**(WFS): `--pnu`에 **법정동 8자리**(시도2+시군구3+읍면동3)를 주면 해당 동 전체 필지를 수집. VWorld WFS는 maxFeatures 1000 cap + startIndex 미지원이라, CLI가 `totalFeatures`로 건수를 보고 PNU 접두를 1000 이하 조각으로 분할(무효 접두 자동 가지치기)해 전수 수집·dedup.
- **`--by-hjd`**(WFS): 법정동 전수(`--all` 내부 사용) → 각 필지 대표점을 역지오코딩해 **행정동(`level4AC`)으로 분류** → `--value-field`(기본 `pblntf_pclnd`=공시지가) 통계(평균·중앙값·Q1·Q3·최저·최고) 출력. **격자 최적화**: 인접 필지를 좌표 격자(`--hjd-grid` 소수자릿수, 기본 3≈100m)로 묶어 **격자당 1회만** 역지오 → 호출 수십~수백 배 절감(16,411필지→648격자→15초). **429 실패분 자동 재처리**(동시성 자동 하향, 최대 6라운드). harvest는 502/연결끊김에 견고 재시도. 출력에 `격자수_역지오호출`·`커버리지`·`비대상_도로하천등`·`미해결에러` 포함.
- **`--input`**(data): PNU 목록 파일을 키풀 병렬로 일괄 조회, `index` 순서 보존 JSON.

## wms / wfs (`/req/wms`, `/req/wfs`)
```
vworld wms --request GetCapabilities
vworld wfs --request GetFeature --typename <레이어> --bbox <...> --max-features 100
```
- WFS는 `output=application/json` 자동 부착(JSON 정규화). bbox는 EPSG:4326 축반전 주의.

## catalog — 다운로드 카탈로그 (`/ned/dtmk/*`)
```
vworld catalog datasets --gid-cd 01 --num-rows 100
vworld catalog gids
vworld catalog gid-datasets --gid-cd 03
```

## staticmap — 정적 지도 이미지 (`/req/image`)
```
vworld staticmap "127.0,37.5" --zoom 14 --size 512,512 --basemap GRAPHIC -o map.png
```
- `--zoom` 6~18, `--size` 최대 1024,1024. basemap: NONE/GRAPHIC/GRAPHIC_WHITE/SATELLITE/HYBRID.

## legend — 범례 이미지 (`/req/image`)
```
vworld legend <레이어> --type ALL -o legend.png       # ALL/LAYER/SUB
```

## tile — 타일 (WMTS/TMS/벡터 통합)
```
vworld tile wmts --layer Base --z 14 --row <Y> --col <X> -o tile.png
vworld tile tms  --layer Base --z 14 --row <Y> --col <X> -o tile.png   # Y축 자동 반전
vworld tile vector --layer Base --z 14 --row <X> --col <Y> -o tile.mvt # MVT(디코딩 안 함)
vworld tile vector-style --layer Base                                  # 스타일 JSON
```
- WMTS layer: Base/white/midnight/Hybrid/Satellite. TMS는 입력 row(WMTS 기준)를 CLI가 자동 반전.

## map — 지도 임베드 생성 (렌더링형)
```
vworld map 2d   --center 127,37.5 -o map.html       # OpenLayers 2D
vworld map 3d   --center 127,37.5 -o globe.html     # WebGL 3D(Cesium)
vworld map 3dsim --center 127,37.5 -o sim.html      # 3D + tool3d 분석
```
- CLI 직접 렌더 불가(Non-Goal) → 스크립트 include URL + 초기화 HTML + 설정 JSON 생성.

## hjd-db — 행정동 경계 SQLite + 지역코드
```
vworld hjd-db build --shp BND_ADM_DONG_PG.shp --db hjd.sqlite   # 경계 SHP→SQLite(폴리곤+bbox 인덱스)
vworld hjd-db region --xlsx "센서스 지역코드.xlsx" --db hjd.sqlite  # 지역코드(시도/시군구/읍면동)→region_code 테이블
vworld hjd-db info --db hjd.sqlite                              # 적재 행정동 수
vworld hjd-db lookup --db hjd.sqlite 야음              # 동명 또는 ADM_CD로 경계+지역코드 조인 조회
vworld hjd-db lookup --db hjd.sqlite 26020610
```
- **두 테이블**: `hjd`(행정동 경계 폴리곤·ADM_CD·ADM_NM), `region_code`(ADM_CD·시도/시군구/읍면동 명칭).
- **조인 키 = ADM_CD(8자리=시도2+시군구3+읍면동3)**. 경계↔지역코드 3,558/3,559(99.97%) 일치(금수면 개명 1건만 출처 드리프트).
- `--sheet`로 다른 연도 시트 선택(기본 2025년 6월, SHP 기준일과 정합).
- VWorld 센서스 행정동경계 SHP(EPSG:5186)를 한 번 적재 → `--hjd-db`로 재사용(129MB SHP 재파싱 불필요).
- 폴리곤은 blob 저장, bbox 컬럼 인덱스로 영역 질의 가속(해당 동 주변 폴리곤만 로드).
- DB·SHP 분류 결과는 동일(검증됨). 대용량 파일은 `.gitignore` 권장.

## config — 키 관리
```
vworld config add-key <KEY> --alias main --referer https://example.com
vworld config list-keys     # 마스킹
vworld config remove-key <KEY|index>
vworld config test-keys     # 실 호출 검증(도메인불일치 가이드)
vworld config path
```
