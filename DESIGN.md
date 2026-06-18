---
omd: 0.1
brand: vworld 3D 분석 지도 UI
bootstrapped_from: toss
bootstrapped_at: 2026-06-17
---

# vworld 3D 분석 지도 — 디자인 시스템 (토스 기반)

vworld 3D 분석·시뮬레이션 15종 HTML의 UI를 토스(Toss) 디자인 시스템 톤으로 통일한다.
**지도(#vmap)가 주인공** — 크고 선명하게, 컨트롤은 토스 카드로 절제되게.

## 1. 색상 (토스 보존)

| 역할 | 값 |
|------|-----|
| Primary (CTA/강조) | `#3182f6` · hover `#2272eb` · 연한 배경 `#e8f3ff` |
| 텍스트 강 (제목) | `#191f28` |
| 텍스트 본문 | `#4e5968` |
| 텍스트 보조 | `#8b95a1` |
| 표면(페이지 배경) | `#f2f4f6` |
| 카드/입력 배경 | `#ffffff` |
| 경계선 | `#e5e8eb` (강조 `#d1d6db`) |
| 성공(초록) | `#03b26c` |
| 위험(빨강) | `#f04452` |
| 주의(주황) | `#fe9800` |

## 2. 타이포 (Pretendard로 토스 Product Sans 대체)

- Font: **Pretendard** (CDN), 가중치 400/600/700 위주, tabular-nums(숫자/결과)
- 제목 17~20px/700, 본문 15~17px/400, 라벨 14px/600, 보조 13px/400

## 3. Radius / Depth

- Radius: 카드/버튼 16, 입력 14, 작은 칩 8, pill 9999
- 그림자(카드): `0 1px 3px rgba(0,0,0,.06), 0 4px 16px rgba(0,0,0,.06)`
- 지도: `0 4px 24px rgba(0,0,0,.10)`

## 4. 컴포넌트

- **버튼(Primary)**: bg `#3182f6`, fg `#fff`, radius 16, 높이 48~52, padding 0 20px, font 16/600, hover `#2272eb`, active scale .98
- **입력/셀렉트**: bg `#fff`, fg `#333d4b`, border `1px #e5e8eb`, radius 14, padding 12px 14px, focus border `#3182f6` + ring `#e8f3ff`
- **카드/패널**: bg `#fff`, radius 16, 그림자, padding 20~24
- **결과 표시**: 카드 안 tabular-nums, 라벨 `#8b95a1` + 값 `#191f28`

## 5. 레이아웃 — 지도 중심

- 페이지: 배경 `#f2f4f6`, 본문 좌우 여백, 최상단 얇은 헤더(분석명)
- **지도 `#vmap`: width 100%, height 70vh(min 520px), radius 16, 그림자, overflow hidden** — 가장 크게
- 컨트롤·결과: 지도 아래(또는 위)에 토스 카드로 배치, 넉넉한 간격(16~24px)
- **지도 내부(vworld 런타임 컨트롤)는 글로벌 스타일에서 격리** (`#vmap` 하위 `all: revert`)

## 6. 절대 보존 (기능 — 변경 금지)

- 모든 `<script src>` (특히 `crossorigin`, `modeler.js`, `webglMapInit`, `@{apikey}`)
- `new vw.CoordZ(...)` 좌표 리터럴 (`--center`/`--address` 치환 지점)
- 모든 element `id`(버튼·입력·결과 span), inline `<script>` 로직
- visiblearea의 page.js DOM stub(`#subContainerToggle` 등)
- → **CSS/레이아웃/지도 크기만** 바꾼다. 마크업 id·스크립트는 그대로.

## 7. Voice

간결·신뢰. 군더더기 없는 한국어 라벨. 결과는 숫자가 또렷하게.
