# VWorld 국가중점데이터(NED) API 카탈로그 — 115건 전수

> 출처: https://www.vworld.kr/dtna/dtna_apiSvcList_s001.do (목록) + `dtna_apiSvcFc_s001.do?apiNum=<N>` (상세) 전수 수집
> 수집일: 2026-06-16 · 수집 방식: 목록 12페이지 + 상세 115건 전부 fetch·파싱
> 이 문서는 `national_data.rs` 구현의 **권위 있는 오퍼레이션 레지스트리**이자 skill `docs/national_data.md`의 기반.

## 0. 요약 통계 (전수 집계)

| 축 | 분포 |
|----|------|
| **총 오퍼레이션** | **115건** (apiNum 1~128, 결번 13개 — 비연속 ID) |
| **대분류 3종** | 부동산 개방데이터 52 · 국가공간 개방데이터 45 · 공간융합 개방데이터 18 |
| **중분류** | 28종 (예: 용도지역지구정보 26건으로 최다, 지가변동률정보 9, 통계성지표정보 9) |
| **엔드포인트 prefix** | `/ned/data/` 46 · `/ned/wms/` 36 · `/ned/wfs/` 33 (합 115, 누락 0) — *권위 집계는 아래 §4 115행 표 기준. 본 요약·문법 절에도 `/ned/..` 문자열이 있어 단순 `grep -c`는 과대계수됨.* |
| **포맷 태그** | XML 79 · JSON 46 · MAP 36 (한 오퍼레이션이 복수 태그 가능) |
| **공통 인증** | `key`(필수) 거의 전수 · `domain`(옵션, 도메인 등록 키 대응) 전수 |

## 1. URL 문법 — `/ned/{wms|wfs|data}/{operation}` 3계열

국가중점데이터는 **단일 엔드포인트가 아니라 균일 문법을 따르는 115개 오퍼레이션 패밀리**다. 데이터 주제(중분류)마다 보통 아래 3변형이 한 세트로 제공된다.

| 계열 | prefix | 응답 | CLI 분류(설계 §1.1) | 대표 파라미터 |
|------|--------|------|---------------------|----------------|
| **WMS조회** | `/ned/wms/{op}WMS` 또는 `…Service` | 맵 이미지(PNG) | **이미지/파일 저장형** | `key, crs, bbox, width, height, format`(+주제별 `stdrYear/stdrMt/reqLvl`) |
| **WFS조회** | `/ned/wfs/get{op}WFS` | GML/XML 또는 JSON | **데이터 반환형** | `key`(필수) + `typename, bbox, pnu, maxFeatures, resultType, srsName, output` (모두 옵션) |
| **속성/목록조회** | `/ned/data/{op}` | XML 또는 JSON | **데이터 반환형** | `key`(필수) + 주제별 키(`pnu` / `admCode` / `stdrYear` …) + `format, numOfRows, pageNo` |

→ **설계 함의: 국가중점데이터는 3분류 중 데이터형·이미지형 2개 분류에 걸친다.** WMS 계열(36건)은 이미지형 파이프라인(바이트 저장), WFS·data 계열(79건)은 데이터형 파이프라인(JSON 정규화)으로 라우팅된다.

## 2. 파라미터 문법 — 상세 페이지 실측 (대표 3건)

### 2.1 속성/data 계열 — `getBuildingAge` (apiNum 3) 실측
| 파라미터 | 필수 | 설명 |
|----------|------|------|
| `pnu` | **필수** | 고유번호(8자리 이상) — 필지고유번호(PNU) |
| `buldAge` | 옵션 | 건물연령 |
| `buldAgeSe` | 옵션 | 건물연령구분(1:이상,2:이하,3:초과,4:미만), 생략 시 `=` 검색 |
| `format` | 옵션 | **응답 형식 `xml` 또는 `json`** ← JSON 네이티브 지원 |
| `numOfRows` | 옵션 | 검색건수(최대 1000) ← **페이지네이션** |
| `pageNo` | 옵션 | 페이지 번호 ← **페이지네이션** |
| `key` | **필수** | 발급 api key |
| `domain` | 옵션 | **API KEY 발급 시 등록한 URL.** "HTTPS·FLEX 등 웹뷰어가 아닌 브라우저에서의 API 사용은 DOMAIN을 추가하여 이용" |

### 2.2 WFS 계열 — `getBuildingAgeWFS` (apiNum 2) 실측
| 파라미터 | 필수 | 설명 |
|----------|------|------|
| `typename` | 옵션 | 질의 대상 피처 유형 이름 리스트(쉼표 구분), [레이어 목록] 참고 |
| `bbox` | 옵션 | 좌표검색 사각형 `(xmin,ymin,xmax,ymax,좌표체계)` — **EPSG:4326은 `(ymin,xmin,ymax,xmax)` 축 순서 주의** |
| `pnu` | 옵션 | 필지고유번호 최소 8자리 (입력 시 **bbox 무시**) |
| `maxFeatures` | 옵션 | 반환 피처 최대값(최대 1000) |
| `resultType` | 옵션 | `results`(완전 응답) / `hits`(개수만) |
| `srsName` | 옵션 | 반환 기하 좌표체계 |
| `output` | 옵션 | 응답 포맷(기본 `text/xml; subtype=gml/2.1.2`) — **`application/json` 지원** (GML2/GML3/json/javascript) |
| `key` | **필수** | 발급 api key |
| `domain` | 옵션 | 도메인 등록 키 대응(2.1과 동일) |

### 2.3 WMS 계열 — `getBuildingAgeWMS` (apiNum 1)
필수: `crs, bbox, width, height, format`(이미지 포맷, png 등)`, key`. 일부 주제(지가변동률 등)는 `stdrYear, stdrMt, reqLvl` 추가 필수.

## 3. 설계 직결 핵심 발견 (→ 본 설계서 §1.1.2 반영)

1. **WFS `output=application/json` 실증** → 설계 §M1 "JSON 우선" 가정이 NED에서 확인됨. 단 **파라미터명은 `output`**(WMS/WFS 표준의 `outputFormat`이 아님). data 계열은 `format=json`.
2. **`domain` 쿼리 파라미터** → 설계 §4.1 도메인 등록 키 인증을 **헤더(Referer)뿐 아니라 쿼리 `domain=`로도** 충족 가능. CLI는 "웹뷰어가 아닌" 호출이므로 `domain` 부착 경로가 1차 권장.
3. **페이지네이션 미설계** → data 계열 `numOfRows(≤1000)/pageNo`, WFS `maxFeatures(≤1000)/resultType=hits`. 대용량 결과는 페이지 순회 필요 — 현재 설계에 없던 신규 요구.
4. **PNU(필지고유번호) 1급 입력** → 18~19자리, 최소 8자리(시도2+시군구3+읍면동3). 다수 속성 API의 필수 키. 엔티티 "필지(Parcel)"의 실 입력 형식.
5. **비연속 apiNum + 레지스트리 필요** → 115개를 손코딩하지 말고 **정적 오퍼레이션 테이블(중분류→{wms,wfs,data 오퍼레이션+파라미터 스펙})** 로 dispatch.

---

## 4. 전수 카탈로그 (115행)

| # | 대분류 | 중분류 | 서비스명 | 포맷 | 엔드포인트(/ned/..) | 필수파라미터 |
|---|--------|--------|----------|------|---------------------|---------------|
| 1 | 공간융합 개방데이터 | 건축물연령정보 | 건축물연령WMS조회 | MAP | /ned/wms/getBuildingAgeWMS | crs, bbox, width, height, format, key |
| 2 | 공간융합 개방데이터 | 건축물연령정보 | 건축물연령WFS조회 | XML | /ned/wfs/getBuildingAgeWFS | key |
| 3 | 공간융합 개방데이터 | 건축물연령정보 | 건축물연령속성조회 | XML/JSON | /ned/data/getBuildingAge | pnu, key |
| 4 | 공간융합 개방데이터 | 용도별건물정보 | 용도별건물WMS조회 | MAP | /ned/wms/getBuildingUseWMS | crs, bbox, width, height, format, key |
| 5 | 공간융합 개방데이터 | 용도별건물정보 | 용도별건물WFS조회 | XML | /ned/wfs/getBuildingUseWFS | key |
| 6 | 공간융합 개방데이터 | 용도별건물정보 | 용도별건물속성조회 | XML/JSON | /ned/data/getBuildingUse | pnu, key |
| 7 | 공간융합 개방데이터 | 지가변동률정보 | 지역별지가변동률WMS조회 | MAP | /ned/wms/getByRegionWMS | crs, bbox, width, height, format, stdrYear, stdrMt, reqLvl |
| 8 | 공간융합 개방데이터 | 지가변동률정보 | 지역별지가변동률속성조회 | XML/JSON | /ned/data/getByRegion | key |
| 9 | 공간융합 개방데이터 | 지가변동률정보 | 권역별 지가변동률속성조회 | XML/JSON | /ned/data/getLargeCLByRegion | key |
| 10 | 공간융합 개방데이터 | 지가변동률정보 | 용도지역별지가변동률WMS조회 | MAP | /ned/wms/getByZoningWMS | crs, bbox, width, height, format, stdrYear, stdrMt, reqLvl |
| 11 | 공간융합 개방데이터 | 지가변동률정보 | 용도지역별지가변동률속성조회 | XML/JSON | /ned/data/getByZoning | key |
| 12 | 공간융합 개방데이터 | 지가변동률정보 | 권역별 용도지역별지가변동률속성조회 | XML/JSON | /ned/data/getLargeCLByZoning | key |
| 13 | 공간융합 개방데이터 | 지가변동률정보 | 이용상황별지가변동률WMS조회 | MAP | /ned/wms/getByLandCategoryWMS | crs, bbox, width, height, format, stdrYear, stdrMt, reqLvl |
| 14 | 공간융합 개방데이터 | 지가변동률정보 | 이용상황별지가변동률속성조회 | XML/JSON | /ned/data/getByLandCategory | key |
| 15 | 공간융합 개방데이터 | 지가변동률정보 | 권역별 이용상황별지가변동률속성조회 | XML/JSON | /ned/data/getLargeCLByLandCategory | key |
| 16 | 공간융합 개방데이터 | 토지특성정보 | 토지특성WMS조회 | MAP | /ned/wms/getLandCharacteristicsWMS | crs, bbox, width, height, format, key |
| 17 | 공간융합 개방데이터 | 토지특성정보 | 토지특성WFS조회 | XML | /ned/wfs/getLandCharacteristicsWFS | key |
| 18 | 공간융합 개방데이터 | 토지특성정보 | 토지특성속성조회 | XML/JSON | /ned/data/getLandCharacteristics | pnu, key |
| 19 | 국가공간 개방데이터 | GIS건물일반집합정보 | GIS건물일반정보WMS조회 | MAP | /ned/wms/getGisGnrlBuildingWMS | crs, bbox, width, height, format, key |
| 20 | 국가공간 개방데이터 | GIS건물일반집합정보 | GIS건물일반정보WFS조회 | XML | /ned/wfs/getGisGnrlBuildingWFS | key |
| 21 | 국가공간 개방데이터 | GIS건물일반집합정보 | GIS건물집합정보WMS조회 | MAP | /ned/wms/getGisAggrBuildingWMS | crs, bbox, width, height, format, key |
| 22 | 국가공간 개방데이터 | GIS건물일반집합정보 | GIS건물집합정보WFS조회 | XML | /ned/wfs/getGisAggrBuildingWFS | key |
| 23 | 국가공간 개방데이터 | 개별공시지가정보 | 개별공시지가WMS조회 | MAP | /ned/wms/getIndvdLandPriceWMS | crs, bbox, width, height, format, key |
| 24 | 국가공간 개방데이터 | 개별공시지가정보 | 개별공시지가WFS조회 | XML | /ned/wfs/getIndvdLandPriceWFS | key |
| 25 | 국가공간 개방데이터 | 개별공시지가정보 | 개별공시지가속성조회 | XML/JSON | /ned/data/getIndvdLandPriceAttr | pnu, key |
| 26 | 국가공간 개방데이터 | 개별주택가격정보 | 개별주택가격WMS조회 | MAP | /ned/wms/getIndvdHousingPriceWMS | crs, bbox, width, height, format, key |
| 27 | 국가공간 개방데이터 | 개별주택가격정보 | 개별주택가격WFS조회 | XML | /ned/wfs/getIndvdHousingPriceWFS | key |
| 28 | 국가공간 개방데이터 | 개별주택가격정보 | 개별주택가격속성조회 | XML/JSON | /ned/data/getIndvdHousingPriceAttr | pnu, key |
| 29 | 국가공간 개방데이터 | 공동주택가격정보 | 공동주택가격WMS조회 | MAP | /ned/wms/getApartHousingPriceWMS | crs, bbox, width, height, format, key |
| 30 | 국가공간 개방데이터 | 공동주택가격정보 | 공동주택가격WFS조회 | XML | /ned/wfs/getApartHousingPriceWFS | key |
| 31 | 국가공간 개방데이터 | 공동주택가격정보 | 공동주택가격속성조회 | XML/JSON | /ned/data/getApartHousingPriceAttr | pnu, key |
| 32 | 국가공간 개방데이터 | 도서(섬)정보 | 도서정보WMS조회 | MAP | /ned/wms/getIslandsWMS | crs, bbox, width, height, format, key |
| 33 | 국가공간 개방데이터 | 도서(섬)정보 | 도서정보WFS조회 | XML | /ned/wfs/getIslandsWFS | key |
| 34 | 국가공간 개방데이터 | 도서(섬)정보 | 도서정보속성조회 | XML/JSON | /ned/data/getIslandsAttr | key |
| 35 | 국가공간 개방데이터 | 부동산개발업정보 | 부동산개발업WMS조회 | MAP | /ned/wms/getEstateDevlopWMS | crs, bbox, width, height, format, key |
| 36 | 국가공간 개방데이터 | 부동산개발업정보 | 부동산개발업WFS조회 | XML | /ned/wfs/getEstateDevlopWFS | key |
| 37 | 국가공간 개방데이터 | 부동산개발업정보 | 부동산개발업기본정보조회 | XML/JSON | /ned/data/getEDBasicInfo | key |
| 38 | 국가공간 개방데이터 | 부동산개발업정보 | 부동산개발업사무소정보조회 | XML/JSON | /ned/data/getEDOfficeInfo | key |
| 39 | 국가공간 개방데이터 | 부동산개발업정보 | 부동산개발업사업실적정보조회 | XML/JSON | /ned/data/getEDBusinessResultsInfo | key |
| 40 | 국가공간 개방데이터 | 부동산개발업정보 | 부동산개발업위반사항정보조회 | XML/JSON | /ned/data/getEDViolationInfo | key |
| 41 | 국가공간 개방데이터 | 부동산중개업정보 | 부동산중개업WMS조회 | MAP | /ned/wms/getEstateBrkpgWMS | crs, bbox, width, height, format, key |
| 42 | 국가공간 개방데이터 | 부동산중개업정보 | 부동산중개업WFS조회 | XML | /ned/wfs/getEstateBrkpgWFS | key |
| 43 | 국가공간 개방데이터 | 부동산중개업정보 | 부동산중개업사무소정보조회 | XML/JSON | /ned/data/getEBOfficeInfo | key |
| 44 | 국가공간 개방데이터 | 부동산중개업정보 | 부동산중개업자정보조회 | XML/JSON | /ned/data/getEBBrokerInfo | key |
| 45 | 국가공간 개방데이터 | 토지소유정보 | 토지소유정보WMS조회 | MAP | /ned/wms/getPossessionWMS | crs, bbox, width, height, format, key |
| 46 | 국가공간 개방데이터 | 토지소유정보 | 토지소유정보WFS조회 | XML | /ned/wfs/getPossessionWFS | key |
| 47 | 국가공간 개방데이터 | 토지소유정보 | 토지소유정보속성조회 | XML/JSON | /ned/data/getPossessionAttr | pnu, key |
| 48 | 국가공간 개방데이터 | 토지이동이력정보 | 토지이동이력속성조회 | XML/JSON | /ned/data/getLandMoveAttr | pnu, key |
| 49 | 국가공간 개방데이터 | 토지이용계획정보 | 토지이용계획WMS조회 | MAP | /ned/wms/getLandUseWMS | crs, bbox, width, height, format, key |
| 50 | 국가공간 개방데이터 | 토지이용계획정보 | 토지이용계획WFS조회 | XML | /ned/wfs/getLandUseWFS | key |
| 51 | 국가공간 개방데이터 | 토지이용계획정보 | 토지이용계획속성조회 | XML/JSON | /ned/data/getLandUseAttr | pnu, key |
| 52 | 국가공간 개방데이터 | 통계성지표정보 | 국토지목별현황조회 | XML/JSON | /ned/data/getAreaOfLandCategory | key |
| 53 | 국가공간 개방데이터 | 통계성지표정보 | 국토지목별토지가격현황조회 | XML/JSON | /ned/data/getPriceOfLandCategory | key |
| 54 | 국가공간 개방데이터 | 통계성지표정보 | 국토소유연령별현황조회 | XML/JSON | /ned/data/getPossessionByAge | key |
| 55 | 국가공간 개방데이터 | 통계성지표정보 | 토지지목변동현황조회 | XML/JSON | /ned/data/getChangeOfLandCategory | key |
| 56 | 국가공간 개방데이터 | 통계성지표정보 | 토지소유자수현황조회 | XML/JSON | /ned/data/getNumberOfOwner | key |
| 57 | 국가공간 개방데이터 | 통계성지표정보 | 토지소유세대수현황조회 | XML/JSON | /ned/data/getNumberOfHouseholds | key |
| 58 | 국가공간 개방데이터 | 통계성지표정보 | 연령대별토지소유현황조회 | XML/JSON | /ned/data/getLandholdingByAge | key |
| 59 | 국가공간 개방데이터 | 통계성지표정보 | 거주지별토지소유현황조회 | XML/JSON | /ned/data/getLandholdingByResidence | key |
| 60 | 국가공간 개방데이터 | 통계성지표정보 | 개별공시지가기본현황조회 | XML/JSON | /ned/data/getIndvdLandPrice | key |
| 72 | 국가공간 개방데이터 | 표준지공시지가정보 | 표준지공시지가WMS조회 | MAP | /ned/wms/getReferLandPriceWMS | crs, bbox, width, height, format, key |
| 73 | 국가공간 개방데이터 | 표준지공시지가정보 | 표준지공시지가WFS조회 | XML | /ned/wfs/getReferLandPriceWFS | key |
| 74 | 국가공간 개방데이터 | 표준지공시지가정보 | 표준지공시지가속성조회 | XML/JSON | /ned/data/getReferLandPriceAttr | key |
| 75 | 부동산 개방데이터 | GIS건물통합정보 | GIS건물통합조회 | MAP | /ned/wms/BldgisSpceService | crs, bbox, width, height, format, key |
| 76 | 부동산 개방데이터 | GIS건물통합정보 | GIS건물통합WFS조회 | XML | /ned/wfs/getBldgisSpceWFS | key |
| 77 | 부동산 개방데이터 | 공유지연명정보 | 공유지연명목록조회 | XML/JSON | /ned/data/cnrdlnList | pnu, key |
| 78 | 부동산 개방데이터 | 대지권등록정보 | 대지권등록목록조회 | XML/JSON | /ned/data/ldaregList | pnu, key |
| 79 | 부동산 개방데이터 | 대지권등록정보 | 건물일련번호조회 | XML/JSON | /ned/data/buldSnList | pnu, key |
| 80 | 부동산 개방데이터 | 대지권등록정보 | 건물동명조회 | XML/JSON | /ned/data/buldCongNmList | pnu, key |
| 81 | 부동산 개방데이터 | 대지권등록정보 | 건물층수조회 | XML/JSON | /ned/data/buldFloorCoList | pnu, key |
| 82 | 부동산 개방데이터 | 대지권등록정보 | 건물호수조회 | XML/JSON | /ned/data/buldHoCoList | pnu, key |
| 83 | 부동산 개방데이터 | 대지권등록정보 | 건물실명조회 | XML/JSON | /ned/data/buldRlnmList | pnu, key |
| 84 | 부동산 개방데이터 | 법정구역정보 | 법정구역도조회 | MAP | /ned/wms/AdresSpceService | crs, bbox, width, height, format, key |
| 85 | 부동산 개방데이터 | 법정구역정보 | 법정구역도WFS조회 | XML | /ned/wfs/getAdresSpceWFS | key |
| 86 | 부동산 개방데이터 | 법정동정보 | 동명조회 | XML/JSON | /ned/data/amdList | admCodeNm, key |
| 87 | 부동산 개방데이터 | 법정동정보 | 시/도조회 | XML/JSON | /ned/data/admCodeList | key |
| 88 | 부동산 개방데이터 | 법정동정보 | 시군구조회 | XML/JSON | /ned/data/admSiList | admCode, key |
| 89 | 부동산 개방데이터 | 법정동정보 | 읍면동조회 | XML/JSON | /ned/data/admDongList | admCode, key |
| 90 | 부동산 개방데이터 | 법정동정보 | 리조회 | XML/JSON | /ned/data/admReeList | admCode, key |
| 91 | 부동산 개방데이터 | 연속지적도형정보 | 연속지적도조회 | MAP | /ned/wms/CtnlgsSpceService | crs, bbox, width, height, format, key |
| 92 | 부동산 개방데이터 | 연속지적도형정보 | 연속지적도WFS조회 | XML | /ned/wfs/getCtnlgsSpceWFS | key |
| 93 | 부동산 개방데이터 | 용도지역지구정보 | 공업주제도조회 | MAP | /ned/wms/IndstrySpceService | crs, bbox, width, height, format, key |
| 94 | 부동산 개방데이터 | 용도지역지구정보 | 공업주제도WFS조회 | XML | /ned/wfs/getIndstrySpceWFS | key |
| 95 | 부동산 개방데이터 | 용도지역지구정보 | 교육문화주제도조회 | MAP | /ned/wms/EdcClturSpceService | crs, bbox, width, height, format, key |
| 96 | 부동산 개방데이터 | 용도지역지구정보 | 교육문화주제도WFS조회 | XML | /ned/wfs/getEdcClturSpceWFS | key |
| 97 | 부동산 개방데이터 | 용도지역지구정보 | 교통주제도조회 | MAP | /ned/wms/TrnsportSpceService | crs, bbox, width, height, format, key |
| 98 | 부동산 개방데이터 | 용도지역지구정보 | 교통주제도WFS조회 | XML | /ned/wfs/getTrnsportSpceWFS | key |
| 99 | 부동산 개방데이터 | 용도지역지구정보 | 국토계획주제도조회 | MAP | /ned/wms/TritPlnSpceService | crs, bbox, width, height, format, key |
| 100 | 부동산 개방데이터 | 용도지역지구정보 | 국토계획주제도WFS조회 | XML | /ned/wfs/getTritPlnSpceWFS | key |
| 101 | 부동산 개방데이터 | 용도지역지구정보 | 국토종합주제도조회 | MAP | /ned/wms/TritGnrlzSpceService | crs, bbox, width, height, format, key |
| 102 | 부동산 개방데이터 | 용도지역지구정보 | 국토종합주제도WFS조회 | XML | /ned/wfs/getTritGnrlzSpceWFS | key |
| 103 | 부동산 개방데이터 | 용도지역지구정보 | 농업주제도조회 | MAP | /ned/wms/FarmngSpceService | crs, bbox, width, height, format, key |
| 104 | 부동산 개방데이터 | 용도지역지구정보 | 농업주제도WFS조회 | XML | /ned/wfs/getFarmngSpceWFS | key |
| 105 | 부동산 개방데이터 | 용도지역지구정보 | 도시주제도조회 | MAP | /ned/wms/CtySpceService | crs, bbox, width, height, format, key |
| 106 | 부동산 개방데이터 | 용도지역지구정보 | 도시주제도WFS조회 | XML | /ned/wfs/getCtySpceWFS | key |
| 107 | 부동산 개방데이터 | 용도지역지구정보 | 산림주제도조회 | MAP | /ned/wms/MtstSpceService | crs, bbox, width, height, format, key |
| 108 | 부동산 개방데이터 | 용도지역지구정보 | 산림주제도WFS조회 | XML | /ned/wfs/getMtstSpceWFS | key |
| 109 | 부동산 개방데이터 | 용도지역지구정보 | 수산주제도조회 | MAP | /ned/wms/MarnSpceService | crs, bbox, width, height, format, key |
| 110 | 부동산 개방데이터 | 용도지역지구정보 | 수산주제도WFS조회 | XML | /ned/wfs/getMarnSpceWFS | key |
| 111 | 부동산 개방데이터 | 용도지역지구정보 | 수자원주제도조회 | MAP | /ned/wms/MarnResrceSpceService | crs, bbox, width, height, format, key |
| 112 | 부동산 개방데이터 | 용도지역지구정보 | 수자원주제도WFS조회 | XML | /ned/wfs/getMarnResrceSpceWFS | key |
| 113 | 부동산 개방데이터 | 용도지역지구정보 | 재난주제도조회 | MAP | /ned/wms/MsfrtnSpceService | crs, bbox, width, height, format, key |
| 114 | 부동산 개방데이터 | 용도지역지구정보 | 재난주제도WFS조회 | XML | /ned/wfs/getMsfrtnSpceWFS | key |
| 117 | 부동산 개방데이터 | 용도지역지구정보 | 지역주제도조회 | MAP | /ned/wms/AreaSpceService | crs, bbox, width, height, format, key |
| 118 | 부동산 개방데이터 | 용도지역지구정보 | 지역주제도WFS조회 | XML | /ned/wfs/getAreaSpceWFS | key |
| 119 | 부동산 개방데이터 | 용도지역지구정보 | 환경에너지주제도조회 | MAP | /ned/wms/EnvrnEnergySpceService | crs, bbox, width, height, format, key |
| 120 | 부동산 개방데이터 | 용도지역지구정보 | 환경에너지주제도WFS조회 | XML | /ned/wfs/getEnvrnEnergySpceWFS | key |
| 121 | 부동산 개방데이터 | 지적도근점정보 | 지적도근점조회 | MAP | /ned/wms/LgstspSpceService | crs, bbox, width, height, format, key |
| 122 | 부동산 개방데이터 | 지적도근점정보 | 지적도근점WFS조회 | XML | /ned/wfs/getLgstspSpceWFS | key |
| 123 | 부동산 개방데이터 | 지적삼각보조점정보 | 지적삼각보조점조회 | MAP | /ned/wms/LgstgsSpceService | crs, bbox, width, height, format, key |
| 124 | 부동산 개방데이터 | 지적삼각보조점정보 | 지적삼각보조점WFS조회 | XML | /ned/wfs/getLgstgsSpceWFS | key |
| 125 | 부동산 개방데이터 | 지적삼각점정보 | 지적삼각점조회 | MAP | /ned/wms/LgstrgSpceService | crs, bbox, width, height, format, key |
| 126 | 부동산 개방데이터 | 지적삼각점정보 | 지적삼각점WFS조회 | XML | /ned/wfs/getLgstrgSpceWFS | key |
| 127 | 부동산 개방데이터 | 토지등급정보 | 토지등급목록조회 | XML/JSON | /ned/data/ladgrdList | pnu, key |
| 128 | 부동산 개방데이터 | 토지임야정보 | 토지임야목록조회 | XML/JSON | /ned/data/ladfrlList | pnu, key |
