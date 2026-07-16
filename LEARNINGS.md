# VWorld CLI — LEARNINGS (자기학습 오답노트)

> 형식: `날짜 | 명령/대상 | 무슨 일 | 왜 | 다음엔 이렇게` — **"왜"가 가장 중요.**
> 규칙: 조회 전 READ, 작업 후 APPEND / 중복금지·보강 / 같은 함정 3회+ 시 일반규칙 승격 / **검증된 것만** 성공으로 기록.

## 일반 규칙 (승격된 함정)

- 주소 지오코딩은 유형이 핵심: 도로명→`--type ROAD`, 지번→`--type PARCEL`. 유형이 틀리면 HTTP 200이지만 `result`가 비어 옴(에러 아님).
- **대량 역지오 배치는 단일 키에서 동시성 2~3** — `--concurrency 12`면 429 레이트리밋으로 절반 이상 실패(망미동 5,798건 중 3,407 실패). 키 1개면 `--concurrency 2~3`이 안정적. 실패분은 에러 인덱스만 추려 재처리(아래 절차 4번). `--by-hjd`는 이 재처리를 자동(최대 6라운드)으로 수행.
- **harvest·역지오 모두 키풀 병렬** — `harvest_wfs_all`은 접두분할 count를 레벨별 병렬, leaf fetch를 키풀 병렬로 수행(에러분 자동 재투입). 키 N개면 라운드로빈 분산으로 거의 선형 가속. 연산동: 키1·harvest순차 14.95초 → 키2·harvest병렬 **4.72초**(3.2배). 키·`--concurrency`를 함께 올려야 효과.
- **센서스 지역코드 xlsx → `region_code` 테이블** — `hjd-db region --xlsx ... --db hjd.sqlite`(calamine로 xlsx 읽기). 시도/시군구/읍면동 명칭+ADM_CD. **경계(hjd)와 ADM_CD(8자리=시도2+시군구3+읍면동3)로 조인** 3,558/3,559(99.97%). `hjd-db lookup`로 코드↔동명↔경계 조회. 미매칭 1건은 금수면→금수강산면 개명 출처 드리프트.
- **★ 행정동경계 SQLite 적재** — `hjd-db build --shp ... --db hjd.sqlite`(3,559 행정동, 0.9초, 폴리곤 blob+bbox 인덱스). `--by-hjd --hjd-db hjd.sqlite`로 재사용(129MB SHP 재파싱 불필요, bbox 질의로 해당 동 폴리곤만 로드). DB·SHP 분류 결과 완전 일치 검증. rusqlite(bundled, SQLite C 내장).
- **★ 최선 = `--hjd-shp <행정동경계.shp>` (point-in-polygon)** — 역지오 0회, 즉시, **경계밖 0·커버리지 100%**(도로도 폴리곤 안), 실제 경계 정확, 오프라인. VWorld 센서스 `BND_ADM_DONG_PG.shp`(EPSG:5186, ADM_NM, 로그인 다운로드). 연산동: 역지오 분류 13,398 → SHP 16,389(+2,991). 좌표변환(TM중부)·PiP 자기완결 구현(`geomath`/`hjd_shp`). 코드체계 주의: SHP ADM_CD(센서스: 부산21/울산26) ≠ level4AC(행안부: 부산26/울산31) — **ADM_NM(행정동명)으로 매칭**.
- **격자 대표가 행정동 없으면 같은 격자 다른 후보로 재시도** — 격자당 후보 5개 보관, 대표가 도로 필지면 다음 후보(대지)로 재역지오. 도로 많은 동(야음동 도로20%)에서 미분류 **47%→24%**(실제 도로/하천 지목 수준), 누락 대지 +1,445 회복. `cell_cands`/`cand_idx`/`exhausted`.
- **격자 대표점은 폴리곤 centroid(정점 평균)** — 첫 꼭짓점은 도로·경계에 걸려 미분류 폭증(연산동 33%). centroid로 바꿔 **미분류 33%→18%**, 분류 필지 +2,467(거의 진짜 누락분). 작은 필지 centroid는 거의 내부에 위치. `centroid_lonlat()`.
- **`--by-hjd`는 격자 분류로 역지오 호출을 대폭 절감** — 인접 필지는 같은 행정동이므로 좌표를 격자(`--hjd-grid` 소수자릿수, 기본 3≈100m)로 양자화해 **격자당 1회만** 역지오. 연산동 16,411필지 → 648격자 → **14.9초**(이전 필지별이면 20-40분). 정밀 필요 시 `--hjd-grid 4`(≈10m).
- **VWorld 서버는 대량 호출 중 간헐적 502/연결끊김** — 단일 호출 실패가 대량 harvest 전체를 abort시킬 수 있음. `harvest_wfs_all`은 각 호출을 `resilient_get_text`(client 내부 3회 위에 추가 6라운드 backoff)로 감싸 흡수. 그래도 서버 장시간 다운이면 회복 후 재실행 필요.
- `--raw`는 데이터형에서 **정규화만 우회**(원 JSON). XML/GML 응답은 기본적으로 JSON 트리로 정규화됨.
- **이미지형 `format`은 png/jpeg/bmp** — 데이터형의 `format=json`과 절대 혼용 금지(중복 주입 시 INVALID_RANGE 에러). 공통 빌더가 format을 자동 주입하지 않고 호출자가 명시.
- **타일은 `key`가 경로에 포함**: `/req/{wmts|tms}/1.0.0/{KEY}/{layer}/{z}/{row}/{col}.{ext}`. 쿼리 `key=`로 주면 404.

## HOW-TO: 행정동별 공시지가(또는 속성) 비교분석

공시지가/PNU는 **법정동 단위**뿐이고 행정동(OO1동/2동)은 **역지오 `level4AC`** 로만 구분된다(상세: `references/docs/haengjeong_api.md`).

**★ 단일 명령(권장)** — 아래 전 절차가 CLI에 내장됨(WFS 전수 → 역지오 분류 → 자동 재처리 → 행정동별 통계):
```bash
vworld ned getIndvdLandPriceWFS --pnu <법정동8자리> --by-hjd            # 공시지가
vworld ned <다른WFS> --pnu <법정동8자리> --by-hjd --value-field <필드>  # 다른 수치필드
```
출력: `행정동별`[{hjd, hjdCode, count, stats{mean,median,q1,q3,min,max}}], `커버리지`, `비대상_도로하천등`, `미해결에러`. 429 실패분은 동시성 자동 하향으로 최대 6라운드 재처리.

**수동 절차**(내부 동작 이해/커스텀용):

```bash
# 0) 행정동코드는 법정동코드 8자리 ≠ 행정동코드(level4AC, 10자리). 법정동 8자리만 알면 됨.
#    법정동코드: ned admSiList --param admCode=<시도2> → 시군구5 → admDongList --param admCode=<시군구5> --param numOfRows=100
# 1) 법정동 전체 필지+공시지가+lon/lat 좌표 전수 수집 (1000 cap 자동 우회)
vworld ned getIndvdLandPriceWFS --pnu <법정동8자리> --all --param srsName=EPSG:4326 > parcels.json
# 2) 각 필지 첫 좌표(lon,lat) 추출 → coords.txt  (index가 곧 필지 순번; 가격은 properties.pblntf_pclnd)
# 3) 좌표 일괄 역지오코딩 → 행정동 분류 (★ 단일 키면 --concurrency 2~3, 12는 429 폭증)
vworld geocode --input coords.txt --reverse --type BOTH --concurrency 3 > hjd.json
# 4) hjd.json items[].error(429)만 인덱스 추려 재처리 → 병합 (커버리지 100%까지 반복)
#    분류 키: result.response.result[].structure.level4A(행정동명)/level4AC(행정동코드)
# 5) index로 parcels(공시지가) ⨝ hjd(level4AC) 조인 → 행정동별 평균/중앙값/최고 집계
```

- **비대상 필지**: 도로·하천은 `level4AC`가 비어 분류 제외(정상). 경계부 소수 필지는 인접 행정동으로 분류될 수 있음.
- **검증 사례**: 부산 망미동(26500101, 5,798필지) → 망미1동/2동 분리 성공. 울산 신정동(31140104, 11,831필지) → 신정1~5동.

## 로그

- 2026-06-16 | geocode "서울특별시 중구 세종대로 110" | 성공: x=126.97835, y=37.56670 반환 | ROAD 유형 + 정식 도로명주소면 정확 | 골든 케이스로 회귀 검증에 사용.
- 2026-06-16 | geocode --input (배치) "관양동 1588-8" | 빈 결과(None) | 지번주소인데 기본 `--type ROAD`로 호출됨 | 지번이 섞인 배치는 PARCEL로 분리하거나, 실패 항목은 PARCEL 재시도.
- 2026-06-16 | config test-keys | 등록 키 "유효" 판정 | 무도메인 키라 referer 없이도 통과 | 도메인 등록 키였다면 referer 필요했을 것 — 거부 시 해결 가이드 출력 확인.
- 2026-06-16 | staticmap | format=json/png 중복 주입으로 INVALID_RANGE | 공통 빌더가 format=json 자동 주입 + 이미지가 format=png 재설정 → 충돌 | 빌더에서 format 자동주입 제거, 호출자 명시로 수정(회귀 테스트 추가).
- 2026-06-16 | tile wmts/tms | WMTS row793 == TMS row1254 **바이트 동일(MD5)** | TMS는 하단기준 Y카운트라 `2^z-1-row` 반전이 같은 지리타일로 매핑됨이 증명 | Y반전 공식 정확 — 골든 동결(§8-⑧). key는 경로에 넣어야 200.
- 2026-06-16 | tile vector | layer=Base는 OWS Exception, **layer=poi/traffic만 유효** | 벡터타일은 배경(Base)이 아닌 주제 레이어(poi/traffic) | 벡터는 `--layer poi|traffic`. 가이드 예시 `poi/11/1746/793.pbf`.
- 2026-06-16 | tile vector vs wmts | 가이드상 WMTS `Base/11/793/1746`, 벡터 `poi/11/1746/793` → **row/col 순서가 서로 반대**(Architect 지적 모순 실측 확정) | VWorld 문서 표기가 WMTS와 벡터에서 상반 | WMTS: --row=Y(793),--col=X(1746). 벡터: --row/--col을 가이드 예시 순서대로(1746/793) 전달. 혼동 주의.
- 2026-06-16 | tile vector-style | 응답이 JSON이 아니라 **JS(PoiStyleFunc 콜백)** | VWorld 스타일은 JS 함수 | --raw로 원문 받아 사용. JSON 파싱 실패 시 문자열로 폴백됨.
- 2026-06-16 | e2e "주소→용도지역" | geocode→data(연속지적도 POINT)로 **PNU 획득**→ned 속성조회 체인 성공 | 속성조회는 PNU 필수인데 geocode는 좌표만 줌 → `data LP_PA_CBND_BUBUN --geom-filter "POINT(x y)"`의 properties.pnu로 연결 | 주소 기반 필지 속성 질의의 표준 3단계 체인으로 승격.
- 2026-06-16 | ned getLandUse(오타) | "알 수 없는 NED 오퍼레이션" | 속성조회 접미사 `Attr` 누락 — 정확명은 `getLandUseAttr` | op명 불확실하면 **먼저 `ned --list`로 레지스트리 확인** 후 호출(자가교정).
- 2026-06-16 | "동 전체 필지 공시지가" | data --emd-cd 단독 실패 / WFS bbox 0건 | 2D data는 geom/attrFilter 필수, WFS는 **`--pnu 법정동8자리`가 동 전체 조회의 정답**(bbox 무시) | 동 단위 전수는 `ned <WFS> --pnu <8자리> --all`. 신정동 11,831필지.
- 2026-06-16 | WFS 1000건 초과 | maxFeatures 1000 cap + **startIndex 미지원** | 오프셋 페이징 불가하나 `totalFeatures`는 cap 무관 진짜 건수 제공 | **PNU 접두 적응분할**로 우회(CLI `--all`에 내장). 무효 접두는 totalFeatures=0으로 자동 가지치기.
- 2026-06-16 | VWorld 에러 JSON 파싱 | text에 이스케이프 안 된 따옴표(`단일검색="Y"`)로 serde 크래시 | VWorld가 비표준 JSON 반환 | 파서에 정규식 salvage 폴백 추가 — 깨진 에러 JSON도 code/text 복원해 정상 에러 처리.
- 2026-06-16 | 행정동 분리 | 공시지가/PNU는 법정동 단위뿐 | VWorld에 행정동 폴리곤 없음 | **역지오 `GetAddress`의 `level4A`(행정동명)/`level4AC`(행정동코드)** 로 분리. 실측: 신정1동=3114051000, 신정2동=3114052000(원응답 확인). references/docs/haengjeong_api.md.
- 2026-06-16 | WFS 좌표계 | `getIndvdLandPriceWFS` 기본 CRS=**EPSG:900913(웹메르카토르)** (좌표 1.4e7대) | srsName 미지정 시 VWorld 기본=900913 | **`--param srsName=EPSG:4326`** 부착 시 lon/lat 직접 반환 → 좌표 변환 불필요.
- 2026-06-16 | geocode 역지오 함정 | `--reverse`인데 `--type` 기본 ROAD면 빈 결과 | 역지오는 type 영향 받음 | 역지오는 **항상 `--type BOTH`**. geocoder 일일 한도 40,000건(공식 가이드).
- 2026-06-22 | `legend`(GetLegendGraphic) | `--style` 미지정/빈값이면 항상 547B "결과없음" PNG → "VWorld 미지원"으로 오판하기 쉬움 | **`style` 파라미터 필수**(공식 가이드 v4dv_legendguide2). WMS GetCapabilities엔 GetLegendGraphic이 안 보이지만 `/req/image` 서비스로는 정상 제공 | **`vworld legend <레이어> --style <레이어명>`**(style은 보통 layer와 동일 이름). 예: `vworld legend lt_c_uq111 --style lt_c_uq111 --type ALL` → 용도지역 범례 11.6KB 정상. type=ALL(레이어+하위)/LAYER/SUB. 레이어·스타일 목록은 WMS/WFS 레퍼런스 참고. **단 이는 VWorld 제공 레이어용** — 사용자 임의 데이터(SGIS 인구 등)는 여전히 `map choropleth --legend`(자체 생성) 사용.
- 2026-07-16 | search DISTRICT/ADDRESS | `--category` 없이 호출 → `PARAM_REQUIRED: category` | VWorld search API가 두 타입은 category 필수(ADDRESS=ROAD/PARCEL, DISTRICT=L1~L4) | `search <q> --type DISTRICT --category L2` / `--type ADDRESS --category PARCEL` 형태로 호출. PLACE/ROAD는 불필요.
- 2026-07-16 | data geomFilter BOX | `BOX(...,EPSG:4326)` EPSG 접미사 포함 → `INVALID_RANGE` | /req/data의 geomFilter는 좌표 4개만 허용(EPSG 접미사 미지원, 4326 고정 해석) | `--geom-filter "BOX(126.977,37.565,126.979,37.567)"` 접미사 없이. POINT/POLYGON도 동일.
- 2026-07-16 | tile vector 축순서 | wmts와 같은 row=Y/col=X로 호출 → ExceptionReport | 벡터(래스터 png·MVT pbf) 엔드포인트는 경로가 z/x/y — **`--row`=X, `--col`=Y로 wmts와 반대** | 실측: z14 서울 wmts=row 6449/col 13969, vector=row 13969/col 6449. USAGE.md 반영.
- 2026-07-16 | tile vector MVT 레이어 | `--layer Base`로 MVT(.pbf) 요청 → `InvalidParameterValue: tiletype [poi, traffic]` | MVT getTile의 tiletype은 **poi·traffic만** 유효, Base는 래스터(--ext png/jpeg) 전용. poi는 z14에서 빈 결과·**z≥15부터 데이터** | MVT는 `--layer poi --z 15+` 또는 `--layer traffic`. 스타일은 `tile vector-style --layer vectorStylePoi`.
- 2026-07-16 | catalog gid-datasets | `--gid-cd 02/03` → "JSON 파싱 실패: control character" | 서버가 dta_dc 값에 이스케이프 안 된 제어문자(개행)를 포함해 반환 — strict serde 거부(01은 정상) | `--raw`로 원응답 수신해 우회. CLI측 제어문자 관용 파싱은 개선 후보.
- 2026-07-16 | 전 명령 실사용 테스트 | README 타일 예시 좌표(z14 row 6729/col 13732)가 TileOutOfRange | 서울 z14 실좌표는 row(Y) 6449 / col(X) 13969 | 테스트 계획·결과: plan/2026-07-16-12:17:49-vworld-전명령-테스트계획.md (76케이스, 이미지·HTML·DXF/SHP 산출물 실검증).
- 2026-07-16 | data geomFilter BOX 광역 | `BOX(126.9,37.4,127.1,37.6)`(400km²) → `INVALID_RANGE: polygon, box경우 요청면적이 10km² 이내` | /req/data geomFilter의 BOX/POLYGON은 **요청면적 10km² 서버 제한**(EPSG 접미사 문제와 별개) | 소구역 분할 조회 또는 동 전수는 `ned <WFS> --pnu <8자리> --all`. 10km² 이내 예: `BOX(126.97,37.55,127.0,37.58)` 정상.
