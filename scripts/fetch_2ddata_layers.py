#!/usr/bin/env python3
"""
VWorld 2D 데이터 API 158개 레이어 속성 스크랩 스크립트
- requests + stdlib html.parser (playwright 없이 동작)
- 체크포인트: scripts/.2ddata_cache/<data_id>.json 에 개별 저장
- 재시도: 페이지당 최대 3회
- 지연: 300~500ms 랜덤, 동시성 1
- 실패 ID: scripts/.2ddata_cache/failed.txt 기록
- 구조이상 가드 3종 로그
"""
import os
import sys
import json
import time
import random
import re
import csv
from pathlib import Path
from html.parser import HTMLParser
from typing import Optional

import requests

# ── 경로 설정 (크로스플랫폼) ──────────────────────────────────────────────────
SCRIPT_DIR = Path(__file__).parent.resolve()
REPO_ROOT = SCRIPT_DIR.parent
SEED_TSV = REPO_ROOT / "skills" / "data" / "twod_seed.tsv"
CACHE_DIR = SCRIPT_DIR / ".2ddata_cache"
FAILED_TXT = CACHE_DIR / "failed.txt"
CATALOG_TSV = REPO_ROOT / "skills" / "data" / "twod_catalog.tsv"
ATTRS_TSV = REPO_ROOT / "skills" / "data" / "twod_attrs.tsv"

LIST_URL = "https://www.vworld.kr/dev/v4dv_2ddataguide2_s001.do"
DETAIL_URL = "https://www.vworld.kr/dev/v4dv_2ddataguide2_s002.do"

# ── HTML 파서 ─────────────────────────────────────────────────────────────────

class TableParser(HTMLParser):
    """속성 테이블 파서: <table> 내 <tr><th>/<td> 텍스트 추출"""
    def __init__(self):
        super().__init__()
        self._in_table = False
        self._table_depth = 0
        self._in_row = False
        self._in_cell = False
        self._cell_text = []
        self._current_row = []
        self.tables = []      # list of list of list of str
        self._current_table = []

    def handle_starttag(self, tag, attrs):
        if tag == "table":
            self._in_table = True
            self._table_depth += 1
            if self._table_depth == 1:
                self._current_table = []
        elif tag == "tr" and self._in_table and self._table_depth == 1:
            self._in_row = True
            self._current_row = []
        elif tag in ("td", "th") and self._in_row:
            self._in_cell = True
            self._cell_text = []

    def handle_endtag(self, tag):
        if tag == "table":
            if self._table_depth == 1 and self._current_table:
                self.tables.append(self._current_table)
            self._table_depth -= 1
            if self._table_depth == 0:
                self._in_table = False
        elif tag == "tr" and self._in_row:
            if self._current_row:
                self._current_table.append(self._current_row)
            self._in_row = False
        elif tag in ("td", "th") and self._in_cell:
            self._current_row.append("".join(self._cell_text).strip())
            self._in_cell = False

    def handle_data(self, data):
        if self._in_cell:
            self._cell_text.append(data)

    def handle_entityref(self, name):
        if self._in_cell:
            mapping = {"nbsp": " ", "amp": "&", "lt": "<", "gt": ">", "quot": '"'}
            self._cell_text.append(mapping.get(name, ""))

    def handle_charref(self, name):
        if self._in_cell:
            try:
                if name.startswith("x"):
                    self._cell_text.append(chr(int(name[1:], 16)))
                else:
                    self._cell_text.append(chr(int(name)))
            except (ValueError, OverflowError):
                pass


class MetaParser(HTMLParser):
    """페이지 상단 한글명·카테고리 추출"""
    def __init__(self):
        super().__init__()
        self._in_h3 = False
        self._in_h4 = False
        self._in_breadcrumb = False
        self._text = []
        self.h3_texts = []
        self.h4_texts = []
        self.title_text = ""

    def handle_starttag(self, tag, attrs):
        attrs_dict = dict(attrs)
        cls = attrs_dict.get("class", "")
        if tag == "h3":
            self._in_h3 = True
            self._text = []
        elif tag == "h4":
            self._in_h4 = True
            self._text = []
        elif tag == "title":
            self._in_breadcrumb = True
            self._text = []

    def handle_endtag(self, tag):
        if tag == "h3" and self._in_h3:
            self.h3_texts.append("".join(self._text).strip())
            self._in_h3 = False
        elif tag == "h4" and self._in_h4:
            self.h4_texts.append("".join(self._text).strip())
            self._in_h4 = False
        elif tag == "title" and self._in_breadcrumb:
            self.title_text = "".join(self._text).strip()
            self._in_breadcrumb = False

    def handle_data(self, data):
        if self._in_h3 or self._in_h4 or self._in_breadcrumb:
            self._text.append(data)


# ── geom 정규화 ───────────────────────────────────────────────────────────────

def normalize_geom(raw: str, data_id: str) -> str:
    """샘플 데이터와 data_id 접두로 {Polygon, Line, Point} 3값 결정"""
    # ag_geom 샘플 기반 추론
    raw_up = raw.upper()
    geom_from_sample = None
    if raw_up:
        if "POINT" in raw_up:
            geom_from_sample = "Point"
        elif "LINE" in raw_up:
            geom_from_sample = "Line"
        elif "POLYGON" in raw_up:
            geom_from_sample = "Polygon"

    # data_id 접두 기반 추론
    uid = data_id.upper()
    if uid.startswith("LP_"):
        geom_from_prefix = "Polygon"
    elif "_C_" in uid:
        geom_from_prefix = "Polygon"
    elif "_L_" in uid:
        geom_from_prefix = "Line"
    elif "_P_" in uid:
        geom_from_prefix = "Point"
    else:
        geom_from_prefix = None

    if geom_from_sample and geom_from_prefix and geom_from_sample != geom_from_prefix:
        print(f"  [WARN] geom 충돌 {data_id}: sample={geom_from_sample}, prefix={geom_from_prefix} → sample 우선")

    return geom_from_sample or geom_from_prefix or "Polygon"


# ── 속성표 파싱 ───────────────────────────────────────────────────────────────

def parse_attr_table(html: str, data_id: str) -> tuple[list[dict], str, str, str]:
    """
    Returns: (attrs, ag_geom_sample, layer_name, category)
    attrs: [{"name","single_search","type","desc"}, ...]
    """
    parser = TableParser()
    parser.feed(html)

    meta_parser = MetaParser()
    meta_parser.feed(html)

    # 한글명: h3 또는 h4에서 추출
    layer_name = ""
    for t in meta_parser.h3_texts + meta_parser.h4_texts:
        t_clean = re.sub(r"\s+", " ", t).strip()
        if t_clean and "속성정보" not in t_clean and "vworld" not in t_clean.lower():
            layer_name = t_clean
            break
    # title 태그에서 보완
    if not layer_name and meta_parser.title_text:
        layer_name = meta_parser.title_text.replace("| VWorld", "").strip()

    # 카테고리: 한글명 첫 토큰 (공백/괄호 앞)
    category = ""
    if layer_name:
        m = re.match(r"([가-힣A-Za-z0-9]+)", layer_name)
        if m:
            category = m.group(1)

    # 속성표 탐색 (헤더: 속성명|단일검색|샘플데이터|설명)
    attrs = []
    ag_geom_sample = ""

    for table in parser.tables:
        if not table:
            continue
        header = [c.replace("*", "").strip() for c in table[0]]
        # 속성명 컬럼 탐색
        name_col = None
        single_col = None
        sample_col = None
        desc_col = None
        for i, h in enumerate(header):
            h_clean = h.replace(" ", "")
            if "속성명" in h_clean:
                name_col = i
            elif "단일검색" in h_clean:
                single_col = i
            elif "샘플데이터" in h_clean or "샘플" in h_clean:
                sample_col = i
            elif "설명" in h_clean:
                desc_col = i

        if name_col is None:
            continue

        for row in table[1:]:
            if len(row) <= name_col:
                continue
            attr_name = row[name_col].strip()
            if not attr_name or attr_name in ("속성명", "합계"):
                continue

            single_val = ""
            if single_col is not None and single_col < len(row):
                single_val = row[single_col].strip()
            single_search = single_val.upper() in ("Y", "예", "O", "TRUE", "✓", "✔")

            sample_val = ""
            if sample_col is not None and sample_col < len(row):
                sample_val = row[sample_col].strip()

            desc_val = ""
            if desc_col is not None and desc_col < len(row):
                desc_val = row[desc_col].strip()

            # type: 설명에서 추론 또는 빈 문자열
            attr_type = ""

            if attr_name == "ag_geom" and sample_val:
                ag_geom_sample = sample_val

            attrs.append({
                "name": attr_name,
                "single_search": single_search,
                "type": attr_type,
                "desc": desc_val,
            })
        if attrs:
            break  # 첫 번째 유효 테이블만

    return attrs, ag_geom_sample, layer_name, category


# ── 구조이상 가드 ─────────────────────────────────────────────────────────────

def guard_check(data_id: str, attrs: list[dict]) -> list[dict]:
    """3종 가드: 중복속성명, single_search 다중, 빈 attrs"""
    # 가드 1: 속성명 중복
    names = [a["name"] for a in attrs]
    seen = set()
    dups = []
    for n in names:
        if n in seen:
            dups.append(n)
        seen.add(n)
    if dups:
        print(f"  [GUARD] 속성명 중복 {data_id}: {dups}")

    # 가드 2: single_search 다중 — 로그만, 데이터는 상세표 그대로 보존(수동확정 대상).
    # 절단하면 실제 다중 단일검색 키(예: ADSIDO ctprvn_cd/ctp_kor_nm)가 손실되므로
    # 상세표의 단일검색* 플래그를 진실의 원천으로 유지한다.
    ss_count = sum(1 for a in attrs if a["single_search"])
    if ss_count > 1:
        ss_names = [a["name"] for a in attrs if a["single_search"]]
        print(f"  [GUARD] single_search 다중 {data_id}: {ss_count}건 {ss_names} → 보존(수동확정)")

    # 가드 3: 속성 0건 (경고만, 빈 배열 허용)
    if not attrs:
        print(f"  [GUARD] 속성 0건 {data_id}")

    return attrs


# ── 세션 생성 ─────────────────────────────────────────────────────────────────

def make_session() -> requests.Session:
    s = requests.Session()
    s.headers.update({
        "User-Agent": (
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
            "AppleWebKit/537.36 (KHTML, like Gecko) "
            "Chrome/124.0.0.0 Safari/537.36"
        ),
        "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        "Accept-Language": "ko-KR,ko;q=0.9,en-US;q=0.8",
        "Referer": LIST_URL,
    })
    # 세션 쿠키 획득
    try:
        r = s.get(LIST_URL, timeout=20)
        r.raise_for_status()
        print(f"  세션 획득 완료 (쿠키: {list(s.cookies.keys())})")
    except Exception as e:
        print(f"  [WARN] 목록 페이지 접근 실패: {e}")
    return s


# ── 단일 페이지 스크랩 ────────────────────────────────────────────────────────

def scrape_one(session: requests.Session, data_id: str, svc_ide: str,
               max_retry: int = 3) -> Optional[dict]:
    url = f"{DETAIL_URL}?svcIde={svc_ide}"
    for attempt in range(1, max_retry + 1):
        try:
            r = session.get(url, timeout=20)
            r.raise_for_status()
            r.encoding = "utf-8"
            html = r.text
            attrs, ag_geom_sample, layer_name, category = parse_attr_table(html, data_id)
            attrs = guard_check(data_id, attrs)
            geom = normalize_geom(ag_geom_sample, data_id)
            return {
                "data_id": data_id,
                "svc_ide": svc_ide,
                "name": layer_name,
                "cat": category,
                "geom": geom,
                "attrs": attrs,
            }
        except Exception as e:
            print(f"  [ERR] {data_id} 시도 {attempt}/{max_retry}: {e}")
            if attempt < max_retry:
                time.sleep(1.0 + random.uniform(0, 1))
    return None


# ── 메인 스크랩 루프 ──────────────────────────────────────────────────────────

def load_seed() -> list[tuple[str, str]]:
    rows = []
    with open(SEED_TSV, encoding="utf-8", newline="") as f:
        reader = csv.reader(f, delimiter="\t")
        next(reader)  # 헤더 스킵
        for row in reader:
            if len(row) >= 2:
                rows.append((row[0].strip(), row[1].strip()))
    return rows


def main():
    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    # 숨김 폴더 속성 (Windows)
    if sys.platform == "win32":
        try:
            import subprocess
            subprocess.run(["attrib", "+h", str(CACHE_DIR)], check=False)
        except Exception:
            pass

    seed = load_seed()
    print(f"시드 로드: {len(seed)}건")

    # 기수집 ID 스킵
    already = set()
    for p in CACHE_DIR.glob("*.json"):
        if p.stem != "failed":
            already.add(p.stem)
    print(f"기수집 스킵: {len(already)}건")

    session = make_session()
    failed = []

    # 기존 failed.txt 로드 (이전 실패 제외한 재시도 대상)
    prev_failed = set()
    if FAILED_TXT.exists():
        for line in FAILED_TXT.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line:
                prev_failed.add(line)

    todo = [(d, s) for d, s in seed if d not in already]
    print(f"스크랩 대상: {len(todo)}건\n")

    for i, (data_id, svc_ide) in enumerate(todo, 1):
        print(f"[{i}/{len(todo)}] {data_id} ({svc_ide})")
        result = scrape_one(session, data_id, svc_ide)

        if result is None:
            print(f"  실패 → failed.txt 기록")
            failed.append(data_id)
        else:
            # 체크포인트 저장
            cache_path = CACHE_DIR / f"{data_id}.json"
            cache_path.write_text(
                json.dumps(result, ensure_ascii=False, indent=2),
                encoding="utf-8"
            )
            print(f"  OK: name={result['name']!r}, geom={result['geom']}, attrs={len(result['attrs'])}건")

        # 페이지 간 300~500ms 랜덤 지연
        if i < len(todo):
            delay = random.uniform(0.3, 0.5)
            time.sleep(delay)

    # 세션 재시작 후 실패분 재시도 (1회)
    if failed:
        print(f"\n--- 실패 {len(failed)}건 재시도 ---")
        session2 = make_session()
        still_failed = []
        for data_id in failed:
            svc_map = {d: s for d, s in seed}
            svc_ide = svc_map.get(data_id, "")
            print(f"  재시도 {data_id}")
            result = scrape_one(session2, data_id, svc_ide, max_retry=2)
            if result is None:
                still_failed.append(data_id)
            else:
                cache_path = CACHE_DIR / f"{data_id}.json"
                cache_path.write_text(
                    json.dumps(result, ensure_ascii=False, indent=2),
                    encoding="utf-8"
                )
                print(f"  재시도 OK: {data_id}")
            time.sleep(random.uniform(0.3, 0.5))
        failed = still_failed

    # failed.txt 갱신
    if failed:
        FAILED_TXT.write_text("\n".join(failed) + "\n", encoding="utf-8")
        print(f"\n실패 {len(failed)}건 → {FAILED_TXT}")
    else:
        if FAILED_TXT.exists():
            FAILED_TXT.unlink()
        print("\n모든 ID 스크랩 성공")

    # ── TSV 생성 ──────────────────────────────────────────────────────────────
    build_tsv(seed)


def build_tsv(seed: list[tuple[str, str]]):
    """캐시 JSON → twod_catalog.tsv + twod_attrs.tsv"""
    print("\n--- TSV 생성 ---")

    catalog_rows = []
    attrs_rows = []
    zero_attrs = []
    dup_names_log = []

    svc_map = {d: s for d, s in seed}

    for data_id, svc_ide in seed:
        cache_path = CACHE_DIR / f"{data_id}.json"
        if cache_path.exists():
            data = json.loads(cache_path.read_text(encoding="utf-8"))
            name = data.get("name", "")
            cat = data.get("cat", "")
            geom = data.get("geom", normalize_geom("", data_id))
            attrs = data.get("attrs", [])
        else:
            # 미수집: 접두 기반 geom, 빈 attrs
            name = ""
            cat = ""
            geom = normalize_geom("", data_id)
            attrs = []
            print(f"  [WARN] 미수집 {data_id} → 빈 attrs")

        # 가드: 속성명 중복 재검사
        names = [a["name"] for a in attrs]
        if len(names) != len(set(names)):
            dup_names_log.append(data_id)

        # 가드: 속성 0건
        if not attrs:
            zero_attrs.append(data_id)

        catalog_rows.append([data_id, svc_ide, name, cat, geom])
        attrs_rows.append([data_id, json.dumps(attrs, ensure_ascii=False)])

    # catalog TSV 쓰기
    with open(CATALOG_TSV, "w", encoding="utf-8", newline="\n") as f:
        f.write("data_id\tsvc_ide\tname\tcat\tgeom\n")
        for row in catalog_rows:
            f.write("\t".join(row) + "\n")

    # attrs TSV 쓰기 (헤더 없음)
    with open(ATTRS_TSV, "w", encoding="utf-8", newline="\n") as f:
        for row in attrs_rows:
            f.write("\t".join(row) + "\n")

    print(f"catalog: {CATALOG_TSV} ({len(catalog_rows)}행)")
    print(f"attrs:   {ATTRS_TSV} ({len(attrs_rows)}행)")

    if zero_attrs:
        print(f"\n[GUARD] 속성 0건 ID ({len(zero_attrs)}건): {zero_attrs}")
    else:
        print("\n[GUARD] 속성 0건: 0건 ✓")

    if dup_names_log:
        print(f"[GUARD] 속성명 중복 ID: {dup_names_log}")
    else:
        print("[GUARD] 속성명 중복: 0건 ✓")


# ── 골든 샘플 검증 ────────────────────────────────────────────────────────────
GOLDEN = {
    "LP_PA_CBND_BUBUN": {"attr_count": 9, "single_search_names": ["pnu"]},
    "LT_C_UQ111": {"attr_count": 6, "single_search_names": []},
    "LT_L_SPRD": {"geom": "Line", "attr_count": 2},
    "LT_P_UTISCCTV": {"geom": "Point", "attr_count": 3},
    "LT_C_ADSIDO_INFO": {
        "attr_count": 4,
        "single_search_names": ["ctprvn_cd", "ctp_kor_nm"],
    },
}


def verify_golden():
    print("\n--- 골든 샘플 검증 ---")
    ok = True
    for data_id, expected in GOLDEN.items():
        cache_path = CACHE_DIR / f"{data_id}.json"
        if not cache_path.exists():
            print(f"  [SKIP] {data_id}: 캐시 없음")
            continue
        data = json.loads(cache_path.read_text(encoding="utf-8"))
        attrs = data.get("attrs", [])
        geom = data.get("geom", "")

        if "attr_count" in expected:
            ac = expected["attr_count"]
            got = len(attrs)
            match = got == ac
            print(f"  {data_id}: 속성수 기대={ac} 실제={got} {'✓' if match else '✗'}")
            if not match:
                ok = False

        if "geom" in expected:
            eg = expected["geom"]
            match = geom == eg
            print(f"  {data_id}: geom 기대={eg} 실제={geom} {'✓' if match else '✗'}")
            if not match:
                ok = False

        if "single_search_names" in expected:
            ss_names = [a["name"] for a in attrs if a.get("single_search")]
            for ssn in expected["single_search_names"]:
                match = ssn in ss_names
                print(f"  {data_id}: single_search={ssn!r} {'✓' if match else f'✗ (실제: {ss_names})'}")
                if not match:
                    ok = False

    print("골든 검증 결과:", "전부 일치 ✓" if ok else "불일치 있음 ✗")


if __name__ == "__main__":
    if "--verify-only" in sys.argv:
        seed = load_seed()
        verify_golden()
        build_tsv(seed)
    else:
        main()
        verify_golden()
