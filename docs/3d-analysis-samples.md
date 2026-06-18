# 3D 분석·시뮬레이션 / 지도 샘플 (`vworld map --analysis`)

vworld CLI는 VWorld 공식 코드샘플을 단일 바이너리에 임베드한다(`include_str!`, 자기완결). `--analysis <키>`로 해당 샘플 HTML을 생성하며, `@{apikey}`는 등록된 키로 자동 치환되고 토스 디자인 시스템 스타일이 주입된다.

## 사용법

```bash
# 전체 목록(키 없이도 동작, 버전 그룹 헤더 표시)
vworld map --analysis list

# 샘플 HTML 생성 → 파일 저장
vworld map --analysis mapcontroller --output map.html

# 위치 지정(재중심): 좌표 또는 주소
vworld map --analysis measure --center 129.16,35.16 --output busan.html
vworld map --analysis measure --address "부산광역시청" --output busan.html

# 원본 HTML을 stdout으로
vworld --raw map --analysis slope
```

### 재중심(`--center` / `--address`) 동작

| API | 재중심 | 동작 |
|-----|--------|------|
| 1.0 / 2.0 | ✅ 지원 | 초기 카메라 좌표(`SAMPLE_CENTER`)를 사용자 좌표로 치환(고도 2000) |
| 3.0 (32종) | ✅ 지원 | 초기 카메라의 첫 `new vw.CoordZ(lon, lat, alt)`에서 **경도·위도만 치환, 고도 유지** |
| 3.0 (`mapcontroller`, `responsive`) | ⚠️ 미지원 | 초기 좌표 리터럴이 없는 구조(`vw.MapControllerOption` + `vw.ol3.CameraPosition`). 재중심 요청 시 stderr 경고 + 응답 메타 `center_applied: false`, `requested_center` 표기(조용한 실패 없음) |

응답 JSON 메타: `center`(적용된 좌표 또는 null), `center_applied`(bool), `requested_center`(미적용 시 요청 좌표).

## 샘플 카탈로그 (총 49종)

### API 1.0 — 분석·시뮬레이션 (11종)
출처: https://www.vworld.kr/dev/v4dv_opn3dsimmapguide_s001.do

| 키 | 설명 |
|----|------|
| `slope` | 경사도 분석 |
| `terrainvolume` | 토공량 분석 |
| `profile` | 지형 단면 분석 |
| `sunlight` | 일조량 분석 |
| `sunlightrights` | 일조권 분석 |
| `sunlightslope` | 일조권 사선제한 분석 |
| `visiblearea` | 가시면적 분석 |
| `viewsurface` | 시곡면 분석 |
| `culheritalter` | 문화재 현상변경 분석 |
| `route` | 드론·차량 모의주행 시뮬레이션 |
| `buildingcontrol` | 건물모델(glb) 편집 |

### API 2.0 — 가시화 (4종)
출처: https://www.vworld.kr/dev/v4dv_opn3dsimmap2guide_s001.do

| 키 | 설명 |
|----|------|
| `heatmap` | Heatmap 가시화 |
| `cluster` | Cluster 가시화 |
| `grid` | Grid 가시화 |
| `hexbin` | Hexbin 가시화 |

### API 3.0 — WebGL 3D지도 (34종)
출처: https://www.vworld.kr/dev/v4dv_opnws3dmap3guide_s001.do

| 키 | 설명 | 재중심 |
|----|------|:---:|
| `responsive` | 반응형 웹에서 3.0 사용 | ⚠️ |
| `lod4texture` | LOD4 텍스쳐 on/off | ✅ |
| `mapcontroller` | 지도 생성(MapController) | ⚠️ |
| `mapoption` | 초기 옵션 지도 생성 | ✅ |
| `moveto` | 좌표 이동·줌레벨 설정 | ✅ |
| `geometry` | 지오메트리 Point/Line/Polygon | ✅ |
| `geometryz` | z좌표 포함 지오메트리 | ✅ |
| `wms` | WMS 정보 표출 | ✅ |
| `buildinginfo` | 건물 클릭 정보 | ✅ |
| `cameraturn` | 카메라 방향 전환 | ✅ |
| `flight` | 비행 시뮬레이션 | ✅ |
| `rotateface` | 회전 정면 관찰 | ✅ |
| `rotateground` | 회전 지면 관찰 | ✅ |
| `driving` | 운전 시뮬레이션 | ✅ |
| `markerevent` | 마커 이벤트 추가 | ✅ |
| `circle` | Circle/CircleZ 지오메트리 | ✅ |
| `regularshape` | RegularShape 지오메트리 | ✅ |
| `specialshape` | SpecialShape 지오메트리 | ✅ |
| `imagesave` | 이미지 저장 API | ✅ |
| `geojson` | Geojson/GML 해석 | ✅ |
| `wfs` | WFS 레이어 생성 | ✅ |
| `glb` | glb/gltf 업로드 | ✅ |
| `wmswfs` | WMS/WFS API 응용 | ✅ |
| `search` | 검색 API 결과 표시 | ✅ |
| `dataapi` | 데이터 API 좌표 객체 생성 | ✅ |
| `wmts` | WMTS 레이어 추가 | ✅ |
| `home` | Home 버튼 위치 이동 | ✅ |
| `measure` | 높이·거리·면적 측정 | ✅ |
| `buildingroll` | 건물 Roll 기능 | ✅ |
| `draw` | 포인트/라인 그리기·삭제·수정 | ✅ |
| `markergroup` | 마커 그룹 관리 | ✅ |
| `boundary` | 화면 바운더리 동서남북 조회 | ✅ |
| `editfeature` | 포인트/라인 편집 | ✅ |
| `popup` | 포인트 선택 팝업 생성·제거 | ✅ |

## 유지보수

- 3.0 샘플 HTML 원본은 `src/cli/tool3d_samples/v3_*.html`에 있으며, `scripts/fetch_3dmap3_samples.py`로 VWorld 가이드 페이지에서 재추출·정규화(프로토콜 상대경로 → `https://`)·검증할 수 있다.
- 레지스트리(`ANALYSES`)와 재중심 로직은 `src/cli/embed_cmds.rs` 참조.
