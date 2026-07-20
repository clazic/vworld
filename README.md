# vworld

[![release](https://github.com/clazic/vworld/actions/workflows/release.yml/badge.svg)](https://github.com/clazic/vworld/actions/workflows/release.yml)
[![version](https://img.shields.io/github/v/release/clazic/vworld?sort=semver)](https://github.com/clazic/vworld/releases/latest)
[![license](https://img.shields.io/github/license/clazic/vworld)](LICENSE)
![platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey)

**VWorld(국가공간정보포털) OpenAPI를 감싼 자기완결 Rust CLI.**

지오코딩 · 역지오코딩 · 장소 검색 · 2D 데이터레이어(158종) · 국가중점데이터(NED 115종) · WMS/WFS · WMTS 타일 · StaticMap · 범례(SLD) · 3D 분석 지도 임베드(15종) · 연속지적도 DXF/SHP 내보내기까지 — 추가 런타임 없이 단일 바이너리 하나로 동작합니다.

> **인증키 필요**: [VWorld 오픈API](https://www.vworld.kr) → 오픈API → 인증키 신청 (본인 발급, 무료)

---

## 설치

### 방법 1 — 원클릭 설치 스크립트 (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.sh | bash
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.ps1 | iex
```

### 방법 2 — 사전빌드 바이너리 직접 다운로드 (GitHub Releases)

바이너리는 저장소에 두지 않고 **GitHub Actions가 소스에서 3-OS(mac·win·linux) 빌드해 [Releases](https://github.com/clazic/vworld/releases/latest) 자산으로 배포**합니다. 항상 최신 버전을 받는 `latest/download` URL:

| OS | 파일 | URL |
|----|------|-----|
| macOS (universal x86_64 + arm64) | `vworld-macos` | https://github.com/clazic/vworld/releases/latest/download/vworld-macos |
| macOS arm64 native | `vworld` | https://github.com/clazic/vworld/releases/latest/download/vworld |
| Linux x86_64 (glibc ≥ 2.34) | `vworld-linux` | https://github.com/clazic/vworld/releases/latest/download/vworld-linux |
| Windows x86_64 | `vworld-windows.exe` | https://github.com/clazic/vworld/releases/latest/download/vworld-windows.exe |

> 특정 버전을 받으려면 `latest/download`를 `download/v0.1.0`처럼 태그 경로로 바꿉니다. 원클릭 스크립트도 `VWORLD_VERSION=v0.1.0`(sh) / `-Version v0.1.0`(ps1)으로 버전을 고정할 수 있습니다.

다운로드 후 실행 권한을 부여합니다. 인증키는 첫 실행 시 `vworld config add-key`로 등록하면 `~/.vworld/config.toml`에 저장됩니다(바이너리 옆에 설정파일을 둘 필요 없음).

```bash
# macOS / Linux
chmod +x vworld-macos
# macOS Gatekeeper 해제 (최초 1회)
xattr -d com.apple.quarantine vworld-macos
```

### 방법 3 — 소스 빌드

```bash
# rustup이 없으면 먼저 설치: https://rustup.rs
git clone https://github.com/clazic/vworld.git
cd vworld
cargo build --release
# 바이너리: target/release/vworld
```

---

## 업데이트

설치한 vworld는 `vworld update`로 GitHub Releases의 최신 버전을 받아 **자가 교체**하고, 이어서 설치된 **스킬 파일(SKILL.md·references 등)도 함께 갱신**합니다. 다운로드한 자산은 릴리스의 `SHA256SUMS`로 검증합니다.

```bash
vworld update                  # 바이너리 교체 + 스킬 파일 갱신 (각각 y/N 확인)
vworld update --check          # 새 버전 존재 여부만 확인 (교체 안 함)
vworld update --version v0.2.0 # 특정 버전으로 교체 (롤백)
vworld update --yes            # 확인 프롬프트 생략 (CI·비대화형)
vworld update --force          # 같은 버전이어도 다시 받아 교체
vworld update --skill-only     # 스킬 파일만 갱신
vworld update --no-skill       # 바이너리만 교체
```

스킬 갱신 대상은 `~/.claude/skills/vworld`, `~/.codex/skills/vworld`, 현재 폴더의 `.claude`/`.codex` 스킬 경로 중 **이미 설치되어 있는 것**뿐입니다. 없으면 건너뜁니다. 비대화형 환경(파이프·CI)에서는 프롬프트가 자동으로 "아니오"가 되어 의도치 않은 교체가 일어나지 않습니다.

평소 명령 실행 시 **하루 1회** 최신 버전을 자동 감지해, 새 버전이 있으면 알림만 출력합니다(다운로드는 하지 않음). 알림은 `stderr`로 나가 명령 출력(JSON 등)을 오염시키지 않습니다. 자동 감지를 끄려면:

```bash
export VWORLD_NO_UPDATE_CHECK=1   # (CI 환경에서는 자동 생략)
```

> **스킬 모드 주의**: 스킬(`~/.claude/skills/vworld/app/`)로 설치한 경우 `update`는 **실행 중인 바이너리 1개**만 교체합니다. OS별 사본·`~/.local/bin` 사본까지 모두 갱신하려면 설치 스크립트를 다시 실행하세요.

---

## VWorld OpenAPI 인증키 발급

이 CLI는 [VWorld](https://www.vworld.kr)(국토교통부 공간정보 오픈플랫폼)의 OpenAPI를 호출하므로 **본인 명의의 인증키**가 필요합니다. 발급은 무료입니다. 이 저장소에는 어떠한 키도 포함되어 있지 않습니다.

1. **회원가입 / 로그인** — [www.vworld.kr](https://www.vworld.kr) 우상단에서 가입합니다.
2. **인증키 신청** — 상단 메뉴 **오픈API → 인증키 신청**(활용신청)으로 이동합니다.
3. **활용 정보 입력**
   - 서비스(시스템) 이름: 임의로 입력 (예: `vworld-cli`)
   - 사용 URL:
     - **서버 / CLI 용도면 도메인 없이(무도메인) 신청** 가능 — CLI 대부분 기능은 무도메인 키로 동작합니다.
     - 웹페이지에 임베드(생성 HTML을 특정 도메인에 게시)할 경우에만 해당 도메인을 등록합니다.
   - 활용 API: 지오코더 / 검색 / 데이터 / WMS·WFS / 지도(2D·3D) 등 필요한 항목 체크 (전체 선택 무방)
4. **발급** — 신청 즉시 `XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX` 형식의 인증키가 발급됩니다.
5. 발급된 키를 아래 **키 설정**으로 등록합니다.

> 도메인 등록 키를 받았다면 등록한 도메인을 `--referer`로 함께 지정해야 합니다(아래 참고). 무도메인 키는 `--referer` 없이 사용합니다.

---

## 키 설정

VWorld 인증키를 한 번 등록하면 이후 모든 명령에 자동 적용됩니다.

```bash
vworld config add-key <발급받은_KEY> --alias main
# 도메인 등록 키라면 referer 추가
vworld config add-key <KEY> --alias main --referer https://your-domain.com

vworld config list-keys        # 마스킹 목록 확인
vworld config remove-key 0     # 인덱스(또는 키 값)로 제거
vworld config test-keys        # 실 호출 유효성 검증
vworld config path             # config.toml 위치 확인
```

### config.toml 위치

설정 파일은 바이너리 위치와 무관하게 **사용자 홈의 `~/.vworld/config.toml`** 에 저장됩니다.

| OS | 기본 설정 경로 |
|----|--------------|
| macOS / Linux | `~/.vworld/config.toml` |
| Windows | `%USERPROFILE%\.vworld\config.toml` |

`--config <경로>`로 다른 파일을 지정할 수도 있습니다(지정 시 해당 파일이 없으면 에러).

---

## 빠른 시작

```bash
# 주소 → 좌표 (도로명/지번 자동 판별)
vworld geocode "세종대로 110"

# 좌표 → 주소 (자동 역지오코딩)
vworld geocode "126.978,37.566"

# 좌표·지번·도로명 통합 1회 조회
vworld geocoder "경상남도 고성군 하이면 덕명리 420-1"

# 장소/건물 검색
vworld search "광화문" --type PLACE

# 2D 데이터레이어 158종 탐색
vworld data layers --search "지적"
vworld data describe LP_PA_CBND_BUBUN

# 국가중점데이터(NED) 목록 & 개별지공시지가 조회
vworld ned --list
vworld ned getIndvdLandPriceWFS --pnu 1168010100 --all

# WMTS 배경지도 타일 저장
vworld tile wmts --layer Base --z 14 --row 6729 --col 13732 -o tile.png

# 지도 이미지 저장
vworld staticmap "127.0,37.5" --zoom 14 --size 512,512 -o map.png

# 연속지적도 DXF / SHP 내보내기
vworld ned getCtnlgsSpceWFS --address "남산공원길 105" --radius 500 --dxf parcels.dxf
vworld ned getCtnlgsSpceWFS --address "남산공원길 105" --radius 500 --shp parcels.shp
```

---

## 주요 기능

| 명령 | 설명 |
|------|------|
| `geocode` | 주소 → 좌표 / 좌표 → 주소 (도로명·지번 자동 판별, 역지오 자동 감지) |
| `geocoder` | 통합 지오코더 — 좌표·지번·도로명을 한 번에 |
| `search` | 장소·행정구역·도로명 검색 |
| `data layers` | 2D 데이터레이어 158종 목록 (키워드/카테고리/지오메트리 타입 필터) |
| `data describe <ID>` | 레이어 속성표·단일검색키·샘플 URL 조회 |
| `data <ID>` | 2D WFS GetFeature 호출 (bbox / 속성 필터 지원) |
| `ned --list` | 국가중점데이터(NED) 115종 오퍼레이션 목록 |
| `ned <operation>` | NED 개별 오퍼레이션 호출 (`--pnu`, `--all`, `--by-hjd`, `--input` 등) |
| `wms` | WMS GetCapabilities / GetMap |
| `wfs` | WFS GetFeature (bbox / typename, `--viewer` HTML 뷰어) |
| `tile wmts` | WMTS 배경지도 타일 저장 |
| `tile wmts-themes` | WMTS 주제도·해외위성 시계열 타일 |
| `tile wmts-capabilities` | WMTS 능력문서(XML) 저장 |
| `staticmap` | 정적 지도 이미지 저장 (PNG) |
| `legend <layer>` | 범례 이미지 저장 / `--sld` 로 SLD 스타일 XML 저장 |
| `map 2d` | WebGL 3D엔진(Cesium) 평면 모드 HTML |
| `map ol` | OpenLayers 2D — GeoJSON·폴리곤·KML 벡터 오버레이 |
| `map marker` | 마커 + 팝업 지도 HTML |
| `map chart` | 위치 기반 차트(막대/누적/파이) 지도 HTML |
| `map theme` | WMS 주제도 토글 지도 HTML |
| `map text` | 대량 포인트(클러스터링) 지도 HTML |
| `map 3d` | 3D 지구본 HTML |
| `map 3dsim --analysis <type>` | 3D 분석 시뮬레이션 HTML 15종 (경사도·토공량·일조·가시면적 등) |
| `catalog datasets` | 다운로드 카탈로그 조회 |
| `config` | 키 관리 (add-key / list-keys / remove-key / test-keys / path) |
| `hjd-db build` | 행정동 경계 DB 생성 (--by-hjd 고속화용, 선택) |

### WMS/WFS 지원 좌표계

`wms`/`wfs` 명령의 `--crs`에 지정할 수 있는 좌표계 ([공식 가이드](https://www.vworld.kr/dev/v4dv_wmsguide2_s001.do) 기준):

| 좌표계 | EPSG 코드 |
|--------|-----------|
| WGS84 경위도 | **EPSG:4326** (WMS 기본값) |
| GRS80 경위도 | EPSG:4019 |
| Google Mercator | EPSG:3857, **EPSG:900913** (WFS 기본값) |
| 서부원점(GRS80) | EPSG:5180(50만), EPSG:5185 |
| 중부원점(GRS80) | EPSG:5181(50만), EPSG:5186 |
| 제주원점(GRS80, 55만) | EPSG:5182 |
| 동부원점(GRS80) | EPSG:5183(50만), EPSG:5187 |
| 동해(울릉)원점(GRS80) | EPSG:5184(50만), EPSG:5188 |
| UTM-K(GRS80) | EPSG:5179 |

> **bbox 축순서 주의**: WMS는 `EPSG:4326·5185·5186·5187·5188`일 때 `(ymin,xmin,ymax,xmax)` = 위도 먼저. WFS는 `EPSG:4326`일 때만 반전.

**CLI 기본 좌표계** (`--crs` 미지정 시): `geocode`·`search`·`data`·`wms`·`wfs`·`staticmap`은 **EPSG:4326**, `ned`는 **EPSG:5187**(동부원점 TM — 중부는 5186, 서부는 5185로 변경), `map` 계열 입력 좌표는 EPSG:4326(lon,lat).

### 3D 분석 시뮬레이션 (15종)

`vworld map 3dsim --analysis list` 로 전체 목록 확인.

| 분석 유형 | 키 |
|----------|----|
| 경사도 | `slope` |
| 토공량(성토/절토) | `terrainvolume` |
| 지형단면 | `profile` |
| 일조량 | `sunlight` |
| 일조권 | `sunlightrights` |
| 일조사선제한 | `sunlightslope` |
| 가시면적 | `visiblearea` |
| 시곡면 | `viewsurface` |
| 문화재현상변경 | `culheritalter` |
| 드론·차량주행 | `route` |
| 건물편집 | `buildingcontrol` |
| 히트맵 | `heatmap` |
| 클러스터 | `cluster` |
| 그리드 | `grid` |
| 헥스빈 | `hexbin` |

```bash
vworld map 3dsim --analysis slope --address "남산공원길 105" -o slope.html
vworld map 3dsim --analysis sunlight --center 127.0,37.5 -o sunlight.html
```

> 분석 결과값(경사도 분포·성토량 등)은 브라우저(Cesium/WebGL)에서만 계산됩니다. 생성된 HTML을 브라우저로 직접 열어 확인하거나, 아래 Playwright로 수치를 자동 추출할 수 있습니다.

### Playwright (선택 — 3D 분석 결과값 자동 추출)

**왜 필요한가**: `geocode`·`search`·`data`·`ned`·`tile`·`staticmap`·지적도 `--dxf/--shp` 등 **CLI의 일반 기능에는 Playwright가 필요 없습니다**(REST 응답을 바로 받습니다). 다만 `map 3dsim`·2.0 가시화(히트맵·클러스터 등)가 만드는 HTML은 경사도 분포·토공량 같은 수치를 **브라우저의 Cesium/WebGL이 렌더링하면서 계산**합니다. 따라서 이 값을 **사람이 브라우저로 열지 않고 자동으로 추출**하려면 헤드리스 브라우저인 Playwright가 필요합니다.

**설치** (필요한 경우에만):

- **Claude Code 사용자(권장)**: Playwright MCP를 연결하면 별도 설치 없이 생성 HTML에서 결과값을 자동 추출합니다.
- **직접 설치** — 크로미움 브라우저 엔진을 내려받습니다(macOS·Windows·Linux 공통):

```bash
# Node.js 환경
npx playwright install chromium

# 또는 Python 환경
pip install playwright
python -m playwright install chromium
```

> Linux 서버(헤드리스)에서는 크로미움 구동에 시스템 라이브러리가 추가로 필요할 수 있습니다: `npx playwright install-deps`(Debian/Ubuntu) 또는 배포판 패키지로 설치하세요. 결과값 자동 추출이 필요 없다면 이 단계는 건너뛰어도 됩니다.

---

## 데이터 자원

### 빌드타임 임베드 (별도 파일 불필요)

2D 레이어 카탈로그(158종), NED 오퍼레이션(115종), 속성 정의 등 TSV 데이터는 빌드타임에 바이너리에 임베드됩니다. 런타임에 외부 파일을 참조하지 않아 **단일 바이너리만으로 자기완결**합니다.

### vworld.sqlite (opt-in, 약 132 MB)

`--by-hjd`(행정동별 분류·통계) 고속화용 선택적 DB입니다. **없어도 동작**합니다 — 미사용 시 역지오코딩 폴백으로 정상 작동(느리지만 결과 동일).

고속화가 필요하면 행정동 경계 SHP로 1회 생성:

```bash
vworld hjd-db build --shp <행정동경계.shp> --db vworld.sqlite
vworld ned getIndvdLandPriceWFS --pnu 1168010100 --by-hjd --hjd-db vworld.sqlite
```

---

## 전역 옵션

```
vworld [전역옵션] <명령> [인자]

--pretty          JSON 들여쓰기 출력
--raw             원응답(가공 없이) 출력
--concurrency N   병렬 요청 수 (키 풀 자동 분산)
--timing          요청 시간 출력
--referer <URL>   도메인 등록 키용 Referer 헤더
--config <path>   config.toml 경로 지정
```

---

## 플랫폼별 주의사항

### macOS

최초 실행 시 Gatekeeper 경고가 뜰 수 있습니다. 격리 속성을 제거하면 해결됩니다.

```bash
xattr -d com.apple.quarantine /path/to/vworld-macos
```

또는 시스템 환경설정 → 개인 정보 보호 및 보안 → "확인 없이 열기"를 클릭해도 됩니다.

### Windows

PowerShell 실행 정책 오류가 발생하면:

```powershell
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
```

SmartScreen 경고가 뜨면 "추가 정보" → "실행" 을 클릭합니다.

`.exe` 확장자 없이 `vworld`로 호출하려면 PATH 추가가 필요합니다.

```powershell
# 영구 PATH 추가 (PowerShell)
$env:Path += ";$env:LOCALAPPDATA\vworld"
[Environment]::SetEnvironmentVariable("Path", $env:Path, "User")
```

### Linux

glibc 2.34 미만(Ubuntu 20.04 등 구형) 환경에서는 사전빌드 바이너리가 동작하지 않을 수 있습니다. 이 경우 소스에서 직접 빌드하세요.

```bash
# Ubuntu 20.04 소스 빌드
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
git clone https://github.com/clazic/vworld.git && cd vworld
cargo build --release
```

---

## 라이선스 & 이용약관

- 코드: [MIT License](LICENSE)
- **VWorld 데이터**: 국토교통부 VWorld API 이용약관 준수 필요 — https://www.vworld.kr/v4po_main.do
- 인증키는 본인이 직접 발급받아야 하며 이 저장소는 어떠한 키도 포함하지 않습니다.
- 이 도구는 VWorld OpenAPI의 비공식 래퍼입니다. 공식 서비스와 무관합니다.
