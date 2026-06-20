# VWorld 국가중점데이터(dtna) — 기존 WMS/WFS 조회 가능성 + 파라미터 분석

> 분석 대상: https://www.vworld.kr/dtna/dtna_apiSvcList_s001.do (국가중점데이터 API 오퍼레이션 전체)
> 분석일: 2026-06-16 · 분석 모델: Opus
> 근거: ① 목록 페이지 + 상세 페이지(`dtna_apiSvcFc_s001.do?apiNum=N`) 실 fetch ② 프로젝트 기존 카탈로그(`national_data_catalog.md`, `rest_api_catalog.md`) ③ 빌드된 CLI `app/vworld ned --list`(115건 레지스트리) ④ 소스 `src/api/mod.rs`(`NedBuilder`/`QueryBuilder` 빌더 로직)
> 초점: **dtna의 각 오퍼레이션이 VWorld 범용 WMS(`/req/wms`)·WFS(`/req/wfs`)로 조회 가능한가 + 실제 요청 파라미터**

---

## 1. 핵심 결론 (요약)

**dtna 국가중점데이터의 WMS/WFS 오퍼레이션은 범용 `/req/wms`·`/req/wfs`로 "대체 조회되지 않는다". 별도의 전용 엔드포인트 패밀리 `/ned/{wms|wfs|data}/{operation}`로만 호출된다.**

1. **전용 엔드포인트 확정 (실증)**: 대표 6개 상세페이지(건축물연령 WMS/WFS/속성, 지가변동률 WMS, 개별공시지가 WMS/WFS)를 직접 fetch한 결과, 모든 샘플 요청 URL이 예외 없이 `https://api.vworld.kr/ned/{wms|wfs|data}/{op}` 형태였다. 범용 `/req/wms?LAYERS=...` 형태는 **단 한 건도 나타나지 않았다**.
2. **`/ned`와 `/req`는 코드상으로도 분리**: 프로젝트 소스(`src/api/mod.rs`)는 두 베이스를 상수로 분리한다 — 범용은 `REQ_BASE = .../req`(`QueryBuilder`), NED는 `NED_BASE = .../ned`(`NedBuilder`). 두 빌더는 URL 조립·파라미터 규칙이 서로 다르다.
3. **"WMS조회/WFS조회/속성조회" 3종 세트의 정체**:
   - `~WMS조회` = `/ned/wms/{op}WMS` → **맵 이미지(PNG)**. (이미지형)
   - `~WFS조회` = `/ned/wfs/get{op}WFS` → **GML/XML 벡터, `output=application/json`으로 JSON 가능**. (데이터형)
   - `~속성조회` = `/ned/data/{op}` → **XML/JSON 속성**. **이것은 WFS/WMS가 아니라 별도 속성 API(`/ned/data`)다.**
4. **범용 WMS/WFS가 커버하는 범위는 다른 레이어 카탈로그**: 범용 `/req/wms`·`/req/wfs`는 `LAYERS`/`TYPENAME` 파라미터로 VWorld가 공개한 별도 레이어(예: `lt_c_*` 계열)를 그린다. dtna의 115 오퍼레이션과는 레이어 ID 체계·엔드포인트가 다르며 **1:1 치환 불가**다.
5. **CLI 매핑은 이미 완비**: 본 프로젝트 CLI는 `vworld ned <operation>` 단일 명령으로 115건 전부(wms 36 / wfs 33 / data 46)를 레지스트리 기반 디스패치한다. 범용 `vworld wms`/`vworld wfs`는 `/req` 계열 전용이라 dtna 호출에 쓰지 않는다.
6. **인증·페이지네이션 공통**: 전 계열 `key`(필수) + `domain`(옵션, 도메인 등록 키). data 계열은 `numOfRows(≤1000)/pageNo`, WFS는 `maxFeatures(≤1000)/resultType=hits`로 분량 제어.

> 한 줄 요약: **dtna 오퍼레이션은 범용 WMS/WFS로 못 부르고, `/ned/...` 전용 엔드포인트가 필요하며, "속성조회"는 아예 WFS가 아닌 `/ned/data` 속성 API다. CLI는 `vworld ned`로 전부 지원한다.**

---

## 2. 두 엔드포인트 패밀리 비교 (`/req` 범용 vs `/ned` 전용)

| 구분 | 범용 WMS/WFS (`/req`) | NED 전용 (`/ned`, = dtna 오퍼레이션) |
|------|----------------------|--------------------------------------|
| 베이스 URL | `https://api.vworld.kr/req/{wms\|wfs}` | `https://api.vworld.kr/ned/{wms\|wfs\|data}/{op}` |
| 소스 빌더 | `QueryBuilder` (`REQ_BASE`) | `NedBuilder` (`NED_BASE`) |
| 오퍼레이션 지정 | `request=GetMap/GetFeature` + `layers`/`typename`로 레이어 선택 | **URL 경로의 마지막 세그먼트(op)가 곧 데이터셋** (예: `getBuildingAgeWMS`) |
| 대상 레이어 | VWorld 공개 레이어 카탈로그(LAYERS ID) | 국가중점데이터 115 오퍼레이션 |
| WFS JSON | `outputFormat`(WMS/WFS 표준) | **`output=application/json`** (파라미터명 다름) |
| data(속성) 계열 | 없음(범용엔 속성 전용 API 없음) | `/ned/data/{op}` + `format=json` |
| CLI | `vworld wms`, `vworld wfs` | `vworld ned <op>` |
| dtna 호출 가능? | **불가 (레이어 체계 상이)** | **가능 (전용)** |

**결론**: dtna 셀렉터 `#tabview > div.list.bd.hover > ul` 아래 기능들은 전부 `/ned/...` 전용 오퍼레이션이며, 범용 WMS/WFS의 LAYERS 파라미터로 대체할 수 없다. (확인: 상세페이지 fetch 실증 + 소스 빌더 분리)

---

## 3. 오퍼레이션 유형 분류 (115건 전수, CLI 매핑)

레지스트리(`vworld ned --list`)·`national_data_catalog.md` §4 기준. 분포: **WMS형 36 · WFS형 33 · 속성(data)형 46 = 115**.

| 유형 | 접미/패턴 | 엔드포인트 | 응답 | 범용WMS/WFS 대체 | 호출 CLI |
|------|-----------|------------|------|------------------|----------|
| **WMS형(이미지)** | `~WMS조회`, `~조회`(주제도/지적) | `/ned/wms/{op}` | PNG 맵 이미지 | 불가 | `vworld ned <op>` (이미지 저장) |
| **WFS형(벡터)** | `~WFS조회` | `/ned/wfs/get{op}WFS` | GML/XML, JSON 가능 | 불가 | `vworld ned <op>` (JSON 정규화) |
| **속성형(data)** | `~속성조회`, `~목록조회`, `~현황조회`, `~정보조회` | `/ned/data/{op}` | XML/JSON | 해당없음(WFS/WMS 아님) | `vworld ned <op>` |

### 3.1 데이터셋별 3종 세트 매핑 (대표 발췌)

| 데이터셋(중분류) | WMS(이미지) | WFS(벡터) | 속성(data) |
|------------------|-------------|-----------|------------|
| 건축물연령정보 | `/ned/wms/getBuildingAgeWMS` | `/ned/wfs/getBuildingAgeWFS` | `/ned/data/getBuildingAge` |
| 용도별건물정보 | `/ned/wms/getBuildingUseWMS` | `/ned/wfs/getBuildingUseWFS` | `/ned/data/getBuildingUse` |
| 토지특성정보 | `/ned/wms/getLandCharacteristicsWMS` | `/ned/wfs/getLandCharacteristicsWFS` | `/ned/data/getLandCharacteristics` |
| 개별공시지가정보 | `/ned/wms/getIndvdLandPriceWMS` | `/ned/wfs/getIndvdLandPriceWFS` | `/ned/data/getIndvdLandPriceAttr` |
| 개별주택가격정보 | `/ned/wms/getIndvdHousingPriceWMS` | `/ned/wfs/getIndvdHousingPriceWFS` | `/ned/data/getIndvdHousingPriceAttr` |
| 공동주택가격정보 | `/ned/wms/getApartHousingPriceWMS` | `/ned/wfs/getApartHousingPriceWFS` | `/ned/data/getApartHousingPriceAttr` |
| 토지이용계획정보 | `/ned/wms/getLandUseWMS` | `/ned/wfs/getLandUseWFS` | `/ned/data/getLandUseAttr` |
| 토지소유정보 | `/ned/wms/getPossessionWMS` | `/ned/wfs/getPossessionWFS` | `/ned/data/getPossessionAttr` |
| 표준지공시지가정보 | `/ned/wms/getReferLandPriceWMS` | `/ned/wfs/getReferLandPriceWFS` | `/ned/data/getReferLandPriceAttr` |

> 주의 — 세트가 3종이 아닌 경우:
> - **지가변동률정보**: WFS 없음. WMS(`getByRegionWMS`/`getByZoningWMS`/`getByLandCategoryWMS`) + data 속성만 존재. WMS에 `stdrYear/stdrMt/reqLvl` 추가 필수.
> - **주제도·지적 기준점 계열**(용도지역지구 26건, 지적삼각점 등): WMS형 op명이 `~조회`(예: `IndstrySpceService`, `CtnlgsSpceService`)로 `WMS` 접미가 없지만 `kind=wms`다. WFS는 `get~SpceWFS` 쌍으로 존재.
> - **법정동정보·통계성지표·대지권등록 계열**: WMS/WFS 없이 **data 속성 전용**(예: `admCodeList`, `getAreaOfLandCategory`, `ldaregList`). 지오메트리 없는 코드/통계/목록 API.

(전체 115행 표는 `national_data_catalog.md` §4 참조 — 본 문서와 동일 권위 데이터)

---

## 4. 대표 오퍼레이션 파라미터 상세

상세 페이지 본문 파라미터 표는 JS로 동적 렌더링되어 정적 HTML 파싱으로는 추출되지 않았으나, **엔드포인트 URL은 fetch로 직접 확인**했고, 파라미터 스펙은 `national_data_catalog.md` §2의 상세페이지 실측치를 인용한다. (확인=fetch근거 / 인용=기존 실측 카탈로그)

### 4.1 WMS형 — `getBuildingAgeWMS` (건축물연령, apiNum 1)
- 엔드포인트: `GET /ned/wms/getBuildingAgeWMS` — **fetch 확인됨**
- 응답: PNG 맵 이미지

| 파라미터 | 필수 | 의미 | 비고 |
|----------|------|------|------|
| `key` | 필수 | 발급 API 키 | 공통 |
| `domain` | 옵션 | 도메인 등록 키 | 공통(CLI 권장 부착) |
| `crs` | 필수 | 좌표계 | 예 `EPSG:4326` / `EPSG:3857` |
| `bbox` | 필수 | 영역 `xmin,ymin,xmax,ymax` | 좌표계 축순서 주의 |
| `width`,`height` | 필수 | 출력 픽셀 크기 | |
| `format` | 필수 | 이미지 포맷 | `image/png` 등 |
| `stdrYear`,`stdrMt`,`reqLvl` | (지가변동률 WMS만) 필수 | 기준년/월/요청레벨 | 건축물연령 WMS엔 불요 |

> WMS 고유 파라미터(`crs/bbox/width/height/format`)는 범용 `/req/wms`(`GetMap`)와 형태는 유사하나, **레이어를 `layers=`로 지정하지 않고 URL 경로(op)가 곧 레이어**라는 점이 결정적 차이다.

### 4.2 WFS형 — `getBuildingAgeWFS` (건축물연령, apiNum 2)
- 엔드포인트: `GET /ned/wfs/getBuildingAgeWFS` — **fetch 확인됨**
- 응답: GML/XML(기본) 또는 JSON

| 파라미터 | 필수 | 의미 | 비고 |
|----------|------|------|------|
| `key` | 필수 | API 키 | |
| `typename` | 옵션 | 피처 유형명(쉼표구분) | 레이어 목록 참조 |
| `bbox` | 옵션 | `xmin,ymin,xmax,ymax,좌표계` | **EPSG:4326은 `(ymin,xmin,ymax,xmax)` 축반전** |
| `pnu` | 옵션 | 필지고유번호(≥8자리) | **입력 시 bbox 무시** |
| `maxFeatures` | 옵션 | 최대 피처(≤1000) | 페이지네이션 |
| `resultType` | 옵션 | `results`/`hits`(개수만) | |
| `srsName` | 옵션 | 반환 기하 좌표계 | |
| `output` | 옵션 | 응답 포맷 | **`application/json` 지원** (표준 `outputFormat` 아님) |
| `domain` | 옵션 | 도메인 등록 키 | |

> 표준 OGC WFS와 파라미터명이 다름: JSON은 `outputFormat`이 아니라 **`output`**. CLI는 `output=application/json`을 자동 부착한다(`NedBuilder`).

### 4.3 속성형(data) — `getBuildingAge` (건축물연령, apiNum 3)
- 엔드포인트: `GET /ned/data/getBuildingAge` — **fetch 확인됨**
- 응답: XML 또는 JSON. **WFS/WMS가 아닌 속성 전용 API.**

| 파라미터 | 필수 | 의미 | 비고 |
|----------|------|------|------|
| `pnu` | 필수 | 필지고유번호(≥8자리) | 1급 입력 키 |
| `buldAge` | 옵션 | 건물연령 | 오퍼레이션 고유 |
| `buldAgeSe` | 옵션 | 연령구분(1이상/2이하/3초과/4미만) | 오퍼레이션 고유 |
| `format` | 옵션 | `xml`/`json` | data 계열은 `format`(=`json`) |
| `numOfRows` | 옵션 | 검색건수(≤1000) | 페이지네이션 |
| `pageNo` | 옵션 | 페이지 번호 | 페이지네이션 |
| `key` | 필수 | API 키 | |
| `domain` | 옵션 | 도메인 등록 키 | |

### 4.4 지가변동률 WMS — `getByRegionWMS` (지역별, apiNum 7)
- 엔드포인트: `GET /ned/wms/getByRegionWMS` — **fetch 확인됨**
- WMS 공통(`crs/bbox/width/height/format/key`) + **오퍼레이션 고유 필수 `stdrYear`(기준년), `stdrMt`(기준월), `reqLvl`(요청레벨)**. → 시계열 주제도라 기준시점 파라미터가 추가된다.

### 4.5 개별공시지가 WFS — `getIndvdLandPriceWFS` (apiNum 24)
- 엔드포인트: `GET /ned/wfs/getIndvdLandPriceWFS` — **fetch 확인됨**
- 파라미터 형태는 4.2와 동일(WFS 공통). 필지단위 공시지가 벡터. CLI `--all`/`--by-hjd`로 법정동 전수·행정동 집계 지원.

### 4.6 공통 vs 고유 파라미터 정리

| 구분 | WMS형 | WFS형 | 속성(data)형 |
|------|-------|-------|--------------|
| **공통(전 오퍼)** | `key,domain` | `key,domain` | `key,domain` |
| **계열 공통** | `crs,bbox,width,height,format` | `typename,bbox,pnu,maxFeatures,resultType,srsName,output` | `format,numOfRows,pageNo` |
| **오퍼 고유 예** | 지가변동률: `stdrYear,stdrMt,reqLvl` | (대개 공통만) | 건축물연령: `buldAge,buldAgeSe` / 법정동: `admCode` / 통계: 없음 |

---

## 5. 기존 WMS/WFS 조회 가능성 — 결론표

| 질문 | 답 | 근거 |
|------|----|------|
| dtna WMS조회를 범용 `/req/wms`로 호출 가능? | **불가** | 전용 `/ned/wms/{op}` (fetch 6건 실증) |
| dtna WFS조회를 범용 `/req/wfs`로 호출 가능? | **불가** | 전용 `/ned/wfs/get{op}WFS` (fetch 실증) |
| dtna 속성조회가 WFS/WMS인가? | **아니오** | `/ned/data/{op}` 속성 전용 API |
| 범용 WMS/WFS의 LAYERS로 대체 가능? | **불가** | 레이어 ID 체계·엔드포인트 상이, 1:1 치환 없음 |
| 전용 NED 엔드포인트 필요? | **그렇다** | 115건 전부 `/ned/...` |
| 본 프로젝트 CLI가 지원하나? | **전부 지원** | `vworld ned <op>` 레지스트리 115건(wms36/wfs33/data46) |
| WFS JSON 응답 가능? | **가능** | `output=application/json`(표준 outputFormat 아님) |

---

## 6. CLI 활용 예시 (`vworld ...`)

```bash
# 0) 등록된 115 오퍼레이션 확인
vworld ned --list

# 1) WMS형(이미지) — /ned/wms/getBuildingAgeWMS (전용, 범용 wms와 무관)
vworld ned getBuildingAgeWMS --param crs=EPSG:4326 \
  --param bbox=126.97,37.55,126.99,37.57 --param width=512 --param height=512 \
  --param format=image/png

# 2) WFS형(벡터, JSON 자동) — /ned/wfs/getBuildingAgeWFS
vworld ned getBuildingAgeWFS --pnu 1111018300101970001
vworld ned getIndvdLandPriceWFS --pnu 31140104 --all          # 법정동 전수(1000 cap 자동 우회)
vworld ned getIndvdLandPriceWFS --pnu 26500101 --by-hjd --hjd-db hjd.sqlite  # 행정동 집계

# 3) 속성형(data) — /ned/data/getBuildingAge  (WFS 아님)
vworld ned getBuildingAge --pnu 1111018300101970001
vworld ned getIndvdLandPriceAttr --input pnus.txt --concurrency 6   # PNU 배치

# 4) 지가변동률 WMS — 기준시점 고유 파라미터 필요
vworld ned getByRegionWMS --param crs=EPSG:4326 --param bbox=... \
  --param width=512 --param height=512 --param format=image/png \
  --param stdrYear=2024 --param stdrMt=01 --param reqLvl=3

# (대조) 범용 WMS/WFS — /req 계열. dtna가 아닌 VWorld 공개 레이어용
vworld wms --request GetCapabilities
vworld wfs --request GetFeature --typename <레이어> --bbox <...> --max-features 100
```

> 참고: `--param k=v`는 미수집 파라미터 패스스루(반복 가능). `key`/`domain`은 패스스루 거부(Client가 인증으로 자동 주입).

---

## 7. 미지원/후속 과제

1. **WMS형 36건 이미지 파라미터 1급 플래그 부재**: 현재 WMS 오퍼레이션은 `--param crs=... --param bbox=...` 패스스루로만 호출. `--crs/--bbox/--width/--height/--format` 전용 플래그 + 파일저장(`-o`)을 `ned` 서브명령에 추가하면 UX 개선. (지가변동률 계열은 `stdrYear/stdrMt/reqLvl` 필수 검증도 함께.)
2. **상세페이지 파라미터 동적 렌더링**: `dtna_apiSvcFc_s001.do`의 파라미터 표가 JS 로딩이라 정적 fetch로 본문 추출 불가. 전수 파라미터 검증이 필요하면 헤드리스 브라우저(playwright) 또는 내부 AJAX 엔드포인트 직접 호출이 필요. 현재는 `national_data_catalog.md` §2 실측 3건 + URL fetch로 충분.
3. **WFS `typename` 레이어 목록**: 각 WFS op의 유효 `typename` 카탈로그는 미수집. 대부분 op당 단일 피처라 생략 가능하나, 다중 피처 op는 GetCapabilities류 조회 보강 여지.
4. **data 계열 고유 파라미터 스펙 전수화**: 건축물연령(`buldAge/buldAgeSe`), 지가변동률, 법정동(`admCode/admCodeNm`) 등 op별 고유 파라미터는 일부만 실측. 46개 data op 전수 파라미터 표는 후속 harvest 대상.
5. **WFS↔속성 중복 데이터 선택 가이드**: 동일 데이터셋의 WFS(지오메트리 포함)와 data 속성(지오메트리 없음) 중 용도별 권장 선택 기준 문서화 여지.

---

### 부록 A. 분석 방법·검증 로그
- 목록 페이지(`dtna_apiSvcList_s001.do`) fetch 성공(LEN 31,088), 페이지당 `goDetail(1..10)` 페이징 구조 확인.
- 상세페이지 fetch 6건(apiNum 1·2·3·7·23·24) 전부 200 응답, 샘플 요청 URL이 `https://api.vworld.kr/ned/{wms|wfs|data}/{op}`임을 정규식 추출로 확인. 범용 `/req/...` URL은 미출현.
- `app/vworld ned --list` → `{"count":115,"ok":true}`, kind 분포 wms36/wfs33/data46 (소스 테스트 `prefix_counts_match`와 일치).
- `src/api/mod.rs`: `REQ_BASE`(범용)·`NED_BASE`(전용) 상수 분리, `NedBuilder`가 WFS→`output=application/json`, data→`format=json`, WMS→무(이미지) 자동 분기 확인.
