# 행정동(行政洞) 분리 데이터 취득 — VWorld API 조사 결과

> 질문: "행정동을 분리해서 데이터를 가져오는 API를 찾아라" (예: 법정동 신정동을 신정1~5동으로 구분)
> 조사일: 2026-06-16

## 결론

**VWorld에는 행정동 경계(폴리곤) 데이터셋이 없다.** 행정동은 **역지오코딩(Geocoder GetAddress) 응답의 `level4A`(행정동명)·`level4AC`(행정동코드)** 로만 노출된다. 따라서 좌표 → 역지오코딩 → `level4AC` 로 행정동을 분리한다.

## 1. 조사 결과 (US-001)

| 데이터 | API | 행정동 구분 |
|--------|-----|------------|
| 법정동 경계 | 2D데이터 `LT_C_ADEMD_INFO` | ❌ 법정동만(emd_cd=31140104=신정동) |
| 시도/시군구 | `LT_C_ADSIDO_INFO` 등 | ❌ |
| 행정동 경계셋(LT_C_ADH/ADHMD/THMM_HJD) | — | ❌ **존재하지 않음(ERROR)** |
| **행정동 코드/명** | **Geocoder 역지오 `GetAddress`** | ✅ **`level4A`/`level4AC`** |

- VWorld 무료 데이터의 행정경계는 확인된 범위에서 전부 **법정동 기반**(LT_C_AD…). 행정동 폴리곤 데이터셋은 후보명(LT_C_ADH/ADHMD/THMM_HJD) probe 결과 모두 ERROR.
- **한계**: 2D데이터 158종 전체 카탈로그는 아직 미harvest(`USAGE.md` 참조)이므로 "행정동 경계 완전 부재"는 후보 probe 기반 **추정**이다(NED 115종 카탈로그에는 행정동 폴리곤 없음이 확정). 단 행정동 분리는 역지오 level4AC로 이미 충분.
- 행정동 경계 SHP가 필요하면 외부(통계청 **SGIS** 행정구역경계 / 행정안전부 data.go.kr)를 받아야 함.

## 2. 정답 API (US-002) — 역지오코딩으로 행정동 분리

```
vworld geocode "<lon>,<lat>" --reverse --type BOTH
```

응답 `result[].structure`:
| 필드 | 의미 | 예 |
|------|------|-----|
| `level3` | 법정동 | 태평로1가 / 신정동 |
| **`level4A`** | **행정동명** | **신정2동** |
| **`level4AC`** | **행정동코드(10자리)** | **3114052000** |

**실측 원응답 (2026-06-16, `app/vworld geocode "<x,y>" --reverse --type BOTH`):**
```
입력 129.3068,35.5375 → {"level1":"울산광역시","level4A":"신정1동","level4AC":"3114051000"}
입력 129.302,35.519   → {"level1":"울산광역시","level4A":"신정2동","level4AC":"3114052000"}
```
→ **신정1동 ≠ 신정2동** 이 level4A/level4AC로 명확히 구분됨(US-002 인수조건 충족).

**울산 남구 신정동(법정동 31140104) 행정동코드 (실측):**
| 행정동 | level4AC |
|--------|----------|
| 신정1동 | 3114051000 |
| 신정2동 | 3114052000 |
| 신정3동 | 3114053000 |
| 신정4동 | 3114054000 |
| 신정5동 | 3114055000 |

검증: 신정동 필지 표본(WFS 25점)을 역지오코딩 → 5개 행정동 전부로 분류됨(신정1동 4·2동 4·3동 3·4동 2·5동 2).

## 3. 공시지가를 행정동별로 분리하는 연결 경로 (US-003)

공시지가/PNU는 **법정동 신정동 단위**(`ned getIndvdLandPriceWFS --pnu 31140104 --all`)로만 수집된다. 이를 행정동별로 나누려면 **필지 좌표를 역지오코딩**해 `level4AC`로 분류한다(spatial→행정동 join). 구체 절차는 아래 "전체 파이프라인" 참조.

- **CLI 단독 배치 가능**: `geocode --input <좌표파일> --reverse --type BOTH --concurrency N` 가 **이미 역지오 배치를 지원**(검증됨). 좌표 목록 → 행정동 일괄 분류를 키풀 병렬로 수행. `--type BOTH` 누락 시 기본 `ROAD`라 빈 결과 위험 — **역지오는 반드시 `--type BOTH`**.
- **WFS 좌표계(실측)**: `getIndvdLandPriceWFS`의 기본 반환 CRS는 **EPSG:900913(웹 메르카토르)**(응답 `crs.name=EPSG::900913`, 좌표 예 `[14396043.4, 4238824.2]`). **`--param srsName=EPSG:4326`을 부착하면 lon/lat로 직접 반환**(`[129.32186, 35.54961]`) → **좌표 변환 불필요**.
- **비용**: 필지 수만큼 역지오코딩(신정동 약 1.2만 필지). Geocoder **일일 한도 40,000건**(출처: VWorld Geocoder 가이드 `v4dv_geocoderguide2` "일일 지오코딩 요청건수는 최대 40,000건").
- **한계**: 도로·하천 등 비대상 점은 `level4AC`가 비어 분류 제외(index별 결과에 `행정동=None`).

### 전체 파이프라인 (CLI만으로, 좌표 변환 없이)
```bash
# 1) 법정동 신정동 전체 필지+공시지가+geometry (lon/lat로 직접 받기)
vworld ned getIndvdLandPriceWFS --pnu 31140104 --all --param srsName=EPSG:4326 > parcels.json
# 2) 각 필지 대표점 lon,lat 추출 → coords.txt  (변환 불필요, 첫 좌표만 뽑기)
# 3) 좌표 일괄 역지오코딩 → 행정동(level4AC) 분류
vworld geocode --input coords.txt --reverse --type BOTH --concurrency 8 > hjd.json
# 4) parcels.json(공시지가) ⨝ hjd.json(level4AC) index 조인 → 행정동별 집계
```
