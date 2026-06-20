# VWorld CLI 스킬 — 설치 안내

VWorld OpenAPI(한국 공간정보: 지오코딩·검색·2D데이터·국가중점데이터·WMS/WFS·타일·지도임베드)를 호출하는 **자기완결 CLI 스킬**. Chrome·Node 등 추가 런타임 불필요 — 인터넷 연결과 본인 VWorld 인증키만 있으면 동작합니다.

## 0. 원라이너 설치 (권장)

설치 스크립트가 **Claude Code 스킬 번들 전체**(바이너리·문서·행정동 sqlite 포함)를 GitHub Releases에서 받아 스킬 경로에 배치합니다.

```bash
# macOS / Linux — 기본: user 범위(~/.claude/skills/vworld)
curl -fsSL https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.sh | bash

# 프로젝트(현재 디렉터리)에 설치하려면:
VWORLD_SCOPE=project curl -fsSL https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.sh | bash

# Playwright MCP 까지 함께 설치(3D 분석 결과 자동추출용):
VWORLD_PLAYWRIGHT=1 curl -fsSL https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.sh | bash
```

```powershell
# Windows — 기본: user 범위(%USERPROFILE%\.claude\skills\vworld)
irm https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.ps1 | iex

# 파라미터(project/cli, Playwright)를 주려면 스크립트블록으로:
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.ps1))) -Scope project -Playwright
```

| 설정 | sh 환경변수 | ps1 파라미터 | 값 / 기본 |
|------|-------------|--------------|-----------|
| 설치 범위 | `VWORLD_SCOPE` | `-Scope` | `user`(기본) · `project` · `cli`(단독 바이너리만) |
| Playwright MCP | `VWORLD_PLAYWRIGHT=1` | `-Playwright` | 미지정 시 설치 안 함(대화형 터미널에선 y/N 질문) |
| zip URL 오버라이드 | `VWORLD_ZIP_URL` | `-ZipUrl` | 기본: Releases latest |

> 비대화형 파이프(`curl\|bash`)에서는 환경변수/파라미터가 우선이고, 일반 터미널에서 직접 실행하면 범위·Playwright 여부를 대화형으로 묻습니다. 둘 다 없으면 **user** 범위로 진행합니다.
> `cli` 범위는 스킬이 아니라 단독 CLI 바이너리만 `~/.local/bin`(Windows는 `%LOCALAPPDATA%\vworld`)에 설치합니다.

## 1. 수동 설치 (zip 직접 배치)
zip을 풀면 `skills/` 폴더가 나옵니다.
- **Claude Code 스킬로 사용**: `skills/` 폴더를 스킬 경로에 배치 → 자연어로 호출.
- **단독 CLI로 사용**: `skills/app/`의 OS별 바이너리를 직접 실행.

## 2. OS별 바이너리
| OS | 파일 | 비고 |
|----|------|------|
| macOS | `app/vworld-macos` | Apple Silicon + Intel universal |
| Linux | `app/vworld-linux` | x86_64, musl 정적(의존성 없음) |
| Windows | `app/vworld-windows.exe` | x86_64 |
| (기본) | `app/vworld` | macOS universal 사본(편의용) |

Unix 실행 권한: `chmod +x app/vworld-*`

## 3. API 키 발급 & 등록
1. https://www.vworld.kr → 오픈API → 인증키 신청
2. 등록:
   ```
   ./app/vworld-macos config add-key <발급키> --alias main
   # 도메인 등록 키면:  --referer https://your-domain.com
   ```
   → `app/config.toml`에 저장됩니다. (`config list-keys`로 확인, `config test-keys`로 유효성 점검)

## 4. 빠른 시작
```bash
# 주소 → 좌표 (도로명/지번 자동 판별)
./app/vworld-macos geocode "세종대로 110"
# 좌표·지번·도로명 한 번에
./app/vworld-macos geocoder "경상남도 고성군 하이면 덕명리 420-1"
# 좌표 → 주소 (자동 역지오)
./app/vworld-macos geocode "127.0,37.5"
# 공간데이터 / 국가중점데이터
./app/vworld-macos data LP_PA_CBND_BUBUN --geom-filter "BOX(...)"
./app/vworld-macos ned --list
# 지도 이미지
./app/vworld-macos staticmap "127,37.5" --zoom 14 -o map.png
# 3D 분석 HTML (15종: 경사도·토공량·일조 등)
./app/vworld-macos map 3dsim --analysis list
./app/vworld-macos map 3dsim --analysis slope --address "남산공원길 105" -o slope.html
# 2D 데이터레이어 지도 (OpenLayers — 벡터/마커/차트/주제도/대량포인트)
./app/vworld-macos map ol --center 127,37.5 --zoom 13 --geojson feats.geojson -o ol.html
./app/vworld-macos map marker --points markers.json -o marker.html
./app/vworld-macos map chart --type bar --data chart.json -o chart.html
./app/vworld-macos map theme --layers "도시지역:LT_C_UQ111" -o theme.html
# WFS 피처를 토스 디자인 지도로 보기 (HTML)
./app/vworld-macos wfs --typename lt_c_uq111 --bbox "126.97,37.55,126.99,37.57" -o wfs.html
```
자세한 명령·함정: `SKILL.md`, `references/docs/USAGE.md`, `references/docs/rest_api_catalog.md`.

## 5. 3D 분석 결과값 (선택)
`map 3dsim --analysis`는 분석 **HTML을 생성**합니다. 경사도·토공량 등 결과값은 브라우저(Cesium/WebGL)에서만 계산되므로:
- 생성된 HTML을 브라우저로 열어 직접 지점/영역을 지정하거나,
- Claude Code + Playwright MCP 환경이면 AI가 자동으로 트리거해 결과를 추출합니다.

자세한 내용은 `SKILL.md`의 "결과값 자동 추출" 절 참고.

## 6. 행정동별 분석
`ned ... --by-hjd`(행정동별 분류·통계)는 행정동 경계 DB(`references/data/vworld.sqlite`, 약 132MB)를 사용합니다. **배포본에 포함**되어 있어 추가 작업 없이 바로 동작합니다.
(경계를 직접 갱신하려면: `./app/vworld-macos hjd-db build --shp <행정동경계.shp> --db references/data/vworld.sqlite`)

## 요구사항
- 추가 런타임/의존성 없음(단일 바이너리 자기완결).
- 인터넷 연결 + 본인 VWorld 인증키만 필요.
