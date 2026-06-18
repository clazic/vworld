#!/usr/bin/env python3
"""vWorld WebGL 3D지도 API 3.0 공식 샘플 34종을 추출·정규화·검증해 tool3d_samples/v3_*.html로 저장.

계획서: plan/2026-06-18-13:51:06-3dmap3-34samples.md
추출 경로(실증): POST /dev/v4dv_opnmapexam_s002.do (ppsCtt=API012&exaIde=<EXAID>)
  -> <textarea id="sampleCtt"> 안에 HTML 엔티티 인코딩된 완전 HTML.

크로스플랫폼: 표준 라이브러리(urllib, html, re, os)만 사용.
"""
import html
import os
import re
import sys
import urllib.parse
import urllib.request

BASE = "https://www.vworld.kr/dev/v4dv_opnmapexam_s002.do"
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OUTDIR = os.path.join(ROOT, "src", "cli", "tool3d_samples")

# (EXAID, 정본키, 한글설명) — 계획서 §6 정본 매핑 (키의 단일 출처)
SAMPLES = [
    ("EXAID_00000000000092", "responsive",   "반응형 웹에서 3.0 사용"),
    ("EXAID_00000000000143", "lod4texture",   "LOD4 텍스쳐 on/off"),
    ("EXAID_12000000000001", "mapcontroller", "지도 생성(MapController)"),
    ("EXAID_12000000000002", "mapoption",     "초기 옵션 지도 생성"),
    ("EXAID_12000000000003", "moveto",        "좌표 이동·줌레벨 설정"),
    ("EXAID_12000000000006", "geometry",      "지오메트리 Point/Line/Polygon"),
    ("EXAID_12000000000008", "geometryz",     "z좌표 포함 지오메트리"),
    ("EXAID_12000000000012", "wms",           "WMS 정보 표출"),
    ("EXAID_12000000000014", "buildinginfo",  "건물 클릭 정보"),
    ("EXAID_12000000000015", "cameraturn",    "카메라 방향 전환"),
    ("EXAID_12000000000016", "flight",        "비행 시뮬레이션"),
    ("EXAID_12000000000017", "rotateface",    "회전 정면 관찰"),
    ("EXAID_12000000000018", "rotateground",  "회전 지면 관찰"),
    ("EXAID_12000000000019", "driving",       "운전 시뮬레이션"),
    ("EXAID_12000000000020", "markerevent",   "마커 이벤트 추가"),
    ("EXAID_12000000000022", "circle",        "Circle/CircleZ 지오메트리"),
    ("EXAID_12000000000023", "regularshape",  "RegularShape 지오메트리"),
    ("EXAID_12000000000024", "specialshape",  "SpecialShape 지오메트리"),
    ("EXAID_12000000000025", "imagesave",     "이미지 저장 API"),
    ("EXAID_12000000000121", "geojson",       "Geojson/GML 해석"),
    ("EXAID_12000000000122", "wfs",           "WFS 레이어 생성"),
    ("EXAID_12000000000123", "glb",           "glb/gltf 업로드"),
    ("EXAID_12000000000124", "wmswfs",        "WMS/WFS API 응용"),
    ("EXAID_12000000000125", "search",        "검색 API 결과 표시"),
    ("EXAID_12000000000126", "dataapi",       "데이터 API 좌표 객체 생성"),
    ("EXAID_12000000000127", "wmts",          "WMTS 레이어 추가"),
    ("EXAID_12000000000128", "home",          "Home 버튼 위치 이동"),
    ("EXAID_12000000000129", "measure",       "높이·거리·면적 측정"),
    ("EXAID_12000000000130", "buildingroll",  "건물 Roll 기능"),
    ("EXAID_12000000000131", "draw",          "포인트/라인 그리기·삭제·수정"),
    ("EXAID_12000000000132", "markergroup",   "마커 그룹 관리"),
    ("EXAID_12000000000133", "boundary",      "화면 바운더리 동서남북 조회"),
    ("EXAID_12000000000134", "editfeature",   "포인트/라인 편집"),
    ("EXAID_12000000000135", "popup",         "포인트 선택 팝업 생성·제거"),
]

# 기존 15키(embed_cmds.rs ANALYSES) — 전역 유일성 검사용
EXISTING_KEYS = [
    "slope", "terrainvolume", "profile", "sunlight", "sunlightrights",
    "sunlightslope", "visiblearea", "viewsurface", "culheritalter", "route",
    "buildingcontrol", "heatmap", "cluster", "grid", "hexbin",
]

TEXTAREA_RE = re.compile(r'<textarea[^>]*id="sampleCtt"[^>]*>(.*?)</textarea>', re.S)
PROTO_REL_RE = re.compile(r'(src|href)=(["\'])//')
VMAP_STYLE_RE = re.compile(r'<div[^>]*id=["\']vmap["\'][^>]*style=["\']([^"\']*)["\']', re.I)


def fetch(exaide: str) -> str:
    data = urllib.parse.urlencode({"ppsCtt": "API012", "exaIde": exaide}).encode()
    req = urllib.request.Request(BASE, data=data, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return resp.read().decode("utf-8", errors="replace")


def main() -> int:
    # 0) 키 전역 유일성 사전 검사 (49키)
    new_keys = [k for _, k, _ in SAMPLES]
    all_keys = EXISTING_KEYS + new_keys
    dupes = {k for k in all_keys if all_keys.count(k) > 1}
    if dupes:
        print(f"[FATAL] 키 충돌: {sorted(dupes)}", file=sys.stderr)
        return 1
    print(f"[OK] 키 유일성: 기존 {len(EXISTING_KEYS)} + 신규 {len(new_keys)} = {len(all_keys)}개 모두 유일")

    os.makedirs(OUTDIR, exist_ok=True)
    total_bytes = 0
    failures = []
    print(f"\n{'키':<14} {'바이트':>7}  #vmap 인라인 style")
    print("-" * 70)

    for exaide, key, _desc in SAMPLES:
        try:
            page = fetch(exaide)
        except Exception as e:  # noqa: BLE001
            failures.append((key, f"fetch 실패: {e}"))
            continue

        m = TEXTAREA_RE.search(page)
        if not m:
            failures.append((key, "sampleCtt textarea 부재(로그인/오류 페이지 의심)"))
            continue

        src = html.unescape(m.group(1)).strip()

        # 정규화: 프로토콜 상대경로 (src|href)="//" -> https:// 전수 치환
        src = PROTO_REL_RE.sub(r'\1=\2https://', src)

        # 오류 페이지 정의: 셋 중 하나라도 부재 = 오류
        problems = []
        if "@{apikey}" not in src and "@{apiKey}" not in src:
            problems.append("@{apikey} 부재")
        if 'id="vmap"' not in src and "id='vmap'" not in src:
            problems.append("id=vmap 부재")
        if "</head>" not in src.lower():
            problems.append("</head> 부재")
        if PROTO_REL_RE.search(src):
            problems.append("잔존 // 프로토콜 상대경로")
        if problems:
            failures.append((key, "; ".join(problems)))
            continue

        # #vmap 인라인 style 스캔(M2)
        sm = VMAP_STYLE_RE.search(src)
        style = sm.group(1) if sm else "(없음)"
        flag = ""
        if any(p in style.lower() for p in ("position", "left", "top")):
            flag = "  <-- position/left/top 동반"

        out = os.path.join(OUTDIR, f"v3_{key}.html")
        with open(out, "w", encoding="utf-8") as f:
            f.write(src)
        n = len(src.encode("utf-8"))
        total_bytes += n
        print(f"v3_{key:<11} {n:>7}  {style}{flag}")

    print("-" * 70)
    print(f"성공 {len(SAMPLES) - len(failures)}/{len(SAMPLES)}건, 합계 {total_bytes:,} bytes ({total_bytes/1024:.1f} KB)")
    if failures:
        print(f"\n[실패 {len(failures)}건]", file=sys.stderr)
        for key, why in failures:
            print(f"  - {key}: {why}", file=sys.stderr)
        return 2
    print("\n[OK] 34종 전건 추출·정규화·검증 완료")
    return 0


if __name__ == "__main__":
    sys.exit(main())
