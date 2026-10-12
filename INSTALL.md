# VWorld CLI 스킬 — 설치 안내

VWorld OpenAPI(한국 공간정보: 지오코딩·검색·2D데이터·국가중점데이터·WMS/WFS·타일·지도임베드)를 호출하는 **자기완결 CLI 스킬**. Chrome·Node 등 추가 런타임 불필요 — 인터넷 연결과 본인 VWorld 인증키만 있으면 동작합니다.

## 0. 원라이너 설치 (권장)

설치 스크립트가 **Claude Code 스킬 번들**(바이너리·문서·참조 TSV)을 GitHub Releases에서 받아 스킬 경로에 배치합니다. 다운로드한 파일은 릴리스의 `SHA256SUMS`와 대조해 검증하며, 불일치하면 설치를 중단합니다. 행정동 경계 sqlite는 번들에 포함되지 않습니다(6절 참고).

```bash
# macOS / Linux — 기본: user 범위(~/.claude/skills/vworld)
curl -fsSL https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.sh | bash

# 프로젝트(현재 디렉터리)에 설치하려면:
curl -fsSL https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.sh | VWORLD_SCOPE=project bash

# Playwright MCP 까지 함께 설치(3D 분석 결과 자동추출용):
curl -fsSL https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.sh | VWORLD_PLAYWRIGHT=1 bash
```

> 환경변수는 반드시 파이프 **오른쪽 `bash` 앞**에 둡니다. `VWORLD_SCOPE=project curl ... | bash` 처럼 `curl` 앞에 두면 변수가 `bash`에 전달되지 않습니다.
> 재설치해도 스킬 폴더의 `LEARNINGS.md`(사용자가 추가한 오답노트)는 보존됩니다.

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
| 버전 고정 | `VWORLD_VERSION` | `-Version` | 예: `v0.1.0`. 비우면 latest |

> 바이너리는 git 에 두지 않고 **GitHub Actions 가 src 에서 3-OS(mac·win·linux) 빌드 → Releases 자산**으로 배포합니다. 설치 스크립트는 Releases 에서 받습니다.
> 비대화형 파이프(`curl\|bash`)에서는 환경변수/파라미터가 우선이고, 일반 터미널에서 직접 실행하면 범위(번호 선택)·Playwright 여부를 대화형으로 묻습니다. 둘 다 없으면 **user** 범위로 진행합니다.
> `cli` 범위는 스킬이 아니라 단독 CLI 바이너리만 `~/.local/bin`(Windows는 `%LOCALAPPDATA%\vworld`)에 설치합니다.

## 1. 수동 설치 (zip 직접 배치)
`vworld-skill.zip`을 풀면 `SKILL.md`·`app/`·`references/`가 **루트에 바로** 나옵니다(`skills/` 래퍼 폴더 없음).
- **Claude Code 스킬로 사용**: 풀린 내용 전체를 `~/.claude/skills/vworld/`(또는 프로젝트의 `.claude/skills/vworld/`)에 배치 → 자연어로 호출.
- **단독 CLI로 사용**: `app/`의 OS별 바이너리를 직접 실행.

## 2. OS별 바이너리
| OS | 파일 | 비고 |
|----|------|------|
| macOS | `app/vworld-macos` | Apple Silicon + Intel universal |
| Linux | `app/vworld-linux` | x86_64, musl 정적 링크(glibc 등 시스템 라이브러리 의존 없음) |
| Windows | `app/vworld-windows.exe` | x86_64 |
| (기본) | `app/vworld` | macOS **arm64(Apple Silicon) 전용** 사본(편의용, universal 아님) |

Unix 실행 권한: `chmod +x app/vworld-*`

## 3. API 키 발급 & 등록
1. https://www.vworld.kr → 오픈API → 인증키 신청
2. 등록:
   ```
   ./app/vworld-macos config add-key <발급키> --alias main
   # 도메인 등록 키면:  --referer https://your-domain.com
   ```
   → `~/.vworld/config.toml`(Windows: `%USERPROFILE%\.vworld\config.toml`)에 저장됩니다. 바이너리·스킬 폴더 옆이 아니므로 재설치·스킬 삭제 후에도 유지됩니다. (`config list-keys`로 확인, `config test-keys`로 유효성 점검)

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
`ned ... --by-hjd`(행정동별 분류·통계)는 행정동 경계 DB(`references/data/vworld.sqlite`, 약 132MB)를 사용합니다. 이 DB는 용량 때문에 **배포본에 포함되지 않으므로**, 처음 한 번 직접 생성해야 합니다:
```bash
./app/vworld-macos hjd-db build --shp <행정동경계.shp> --db references/data/vworld.sqlite
```

## 요구사항
- 추가 런타임/의존성 없음(단일 바이너리 자기완결).
- 인터넷 연결 + 본인 VWorld 인증키만 필요.
