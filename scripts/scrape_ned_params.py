#!/usr/bin/env python3
"""
NED 115개 API 요청변수 스크래퍼
- 대상: https://www.vworld.kr/dtna/dtna_apiSvcFc_s001.do?apiNum=<N>
- 파싱: #reqVarList 섹션만 (에러코드 표 제외)
- 출력: skills/data/ned_params.tsv (endpoint_op\t<params_json>)
- 표준 라이브러리만 사용 (urllib, html, re, json, time, subprocess, os, sys)
"""

import json
import os
import re
import subprocess
import sys
import time
from html import unescape
from urllib.request import Request, urlopen
from urllib.error import URLError, HTTPError

BASE_URL = "https://www.vworld.kr/dtna/dtna_apiSvcFc_s001.do?apiNum={apinum}"
DELAY = 0.3   # 요청 간 지연(초)
RETRY = 1     # 실패 시 재시도 횟수

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)
OUTPUT_PATH = os.path.join(PROJECT_ROOT, "skills", "data", "ned_params.tsv")
VWORLD_BIN = os.path.join(PROJECT_ROOT, "target", "release", "vworld")


def get_ned_ops():
    """vworld ned --list JSON에서 (apinum, endpoint_op) 목록 추출"""
    result = subprocess.run(
        [VWORLD_BIN, "ned", "--list"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(f"[ERROR] vworld ned --list 실패: {result.stderr}", file=sys.stderr)
        sys.exit(1)
    data = json.loads(result.stdout)
    ops = [(op["apinum"], op["endpoint_op"]) for op in data["operations"]]
    return ops


def fetch_html(apinum):
    """상세페이지 HTML 취득 (실패 시 재시도)"""
    url = BASE_URL.format(apinum=apinum)
    headers = {
        "User-Agent": (
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
            "AppleWebKit/537.36 (KHTML, like Gecko) "
            "Chrome/120.0.0.0 Safari/537.36"
        ),
        "Accept": "text/html,application/xhtml+xml",
        "Accept-Language": "ko-KR,ko;q=0.9",
    }
    for attempt in range(RETRY + 1):
        try:
            req = Request(url, headers=headers)
            with urlopen(req, timeout=15) as resp:
                raw = resp.read()
                # 인코딩 감지: charset 헤더 또는 meta 태그
                ct = resp.headers.get("Content-Type", "")
                enc_match = re.search(r"charset=([^\s;]+)", ct, re.IGNORECASE)
                encoding = enc_match.group(1) if enc_match else "utf-8"
                try:
                    return raw.decode(encoding, errors="replace")
                except LookupError:
                    return raw.decode("utf-8", errors="replace")
        except (URLError, HTTPError, OSError) as e:
            if attempt < RETRY:
                print(f"  [RETRY] apinum={apinum} 재시도 ({e})", file=sys.stderr)
                time.sleep(1.0)
            else:
                raise


def parse_req_var_list(html):
    """
    #reqVarList 섹션에서 요청변수 파싱.
    에러코드 표(<h5>오류메세지</h5> 하위 <table>)와 완전히 분리됨.

    반환: list of dict {name, required, type, default, desc}
    """
    # reqVarList div 추출
    start = html.find('id="reqVarList"')
    if start == -1:
        return None  # 섹션 없음

    # reqVarList 닫히는 지점: </ul>\n\t\t\t\t</div> 패턴
    # reqVarList 이후 첫 번째 </div> 기준으로 섹션 자름
    block_start = html.find(">", start)
    # ul 블록 추출: reqVarList의 ul 전체
    ul_start = html.find("<ul", block_start)
    if ul_start == -1:
        return None

    # ul 닫기 찾기 (중첩 고려)
    pos = ul_start + 4
    depth = 1
    ul_end = pos
    while depth > 0 and pos < len(html):
        open_pos = html.find("<ul", pos)
        close_pos = html.find("</ul>", pos)
        if close_pos == -1:
            break
        if open_pos != -1 and open_pos < close_pos:
            depth += 1
            pos = open_pos + 3
        else:
            depth -= 1
            ul_end = close_pos + 5
            pos = close_pos + 5

    ul_block = html[ul_start:ul_end]

    # li 항목 분할 (li:not(.nodata) - .nodata 제외)
    # nodata li 패턴: <li class="nodata">
    li_pattern = re.compile(r"<li(?:\s[^>]*)?>", re.DOTALL)
    li_positions = [m.start() for m in li_pattern.finditer(ul_block)]
    if not li_positions:
        return []

    items = []
    for i, lp in enumerate(li_positions):
        end = li_positions[i + 1] if i + 1 < len(li_positions) else len(ul_block)
        li_html = ul_block[lp:end]

        # nodata li 건너뜀
        li_tag_end = li_html.find(">")
        li_tag = li_html[: li_tag_end + 1]
        if 'class="nodata"' in li_tag or "nodata" in li_tag:
            continue

        # 파라미터명: <b id="itemNm">...</b>
        nm_match = re.search(r'<b\s+id="itemNm">(.*?)</b>', li_html, re.DOTALL)
        if not nm_match:
            continue
        name = unescape(nm_match.group(1).strip())

        # 필수여부: <span>필수여부<em>필수</em></span> 또는 <em>옵션</em>
        req_match = re.search(
            r"필수여부\s*<em>(.*?)</em>", li_html, re.DOTALL
        )
        required = False
        if req_match:
            req_text = unescape(req_match.group(1).strip())
            required = req_text == "필수"

        # 설명: <span>설명<em>...</em></span>
        desc_match = re.search(
            r"설명\s*<em>(.*?)</em>", li_html, re.DOTALL
        )
        desc = ""
        if desc_match:
            # HTML 태그 제거
            raw_desc = desc_match.group(1)
            desc = re.sub(r"<[^>]+>", " ", raw_desc)
            desc = re.sub(r"\s+", " ", unescape(desc)).strip()

        # 기본값: <input ... value="...">
        val_match = re.search(
            r'<input[^>]+value="([^"]*)"[^>]*>', li_html, re.DOTALL
        )
        default = unescape(val_match.group(1).strip()) if val_match else ""

        items.append(
            {
                "name": name,
                "required": required,
                "type": "",      # HTML에 타입 정보 없음
                "default": default,
                "desc": desc,
            }
        )

    return items


def main():
    print("=== NED 115개 API 요청변수 스크래퍼 ===")
    print(f"출력: {OUTPUT_PATH}")

    ops = get_ned_ops()
    print(f"총 {len(ops)}개 오퍼레이션 로드 완료\n")

    # (apinum -> endpoint_op) 매핑
    results = {}          # endpoint_op -> params list or {"parse_failed": True}
    failed_ops = []
    success_count = 0
    parse_failed_count = 0
    total_params = 0

    for idx, (apinum, endpoint_op) in enumerate(ops):
        print(f"[{idx+1:3d}/{len(ops)}] apinum={apinum:3d} {endpoint_op}", end="  ")
        sys.stdout.flush()

        try:
            html = fetch_html(apinum)
            time.sleep(DELAY)

            params = parse_req_var_list(html)

            if params is None:
                # reqVarList 섹션 자체가 없음
                print(f"[WARN] reqVarList 섹션 없음 → parse_failed")
                results[endpoint_op] = {"parse_failed": True, "reason": "no_reqVarList"}
                parse_failed_count += 1
                failed_ops.append((apinum, endpoint_op, "no_reqVarList"))
            else:
                param_count = len(params)
                total_params += param_count
                success_count += 1
                print(f"파라미터 {param_count}개")
                results[endpoint_op] = params

        except (URLError, HTTPError, OSError) as e:
            print(f"[ERROR] 네트워크 실패: {e}")
            results[endpoint_op] = {"parse_failed": True, "reason": str(e)}
            parse_failed_count += 1
            failed_ops.append((apinum, endpoint_op, str(e)))
            time.sleep(1.0)

    # TSV 저장
    os.makedirs(os.path.dirname(OUTPUT_PATH), exist_ok=True)
    with open(OUTPUT_PATH, "w", encoding="utf-8", newline="") as f:
        for _, endpoint_op in ops:
            params_val = results.get(endpoint_op, {"parse_failed": True, "reason": "missing"})
            params_json = json.dumps(params_val, ensure_ascii=False, separators=(",", ":"))
            f.write(f"{endpoint_op}\t{params_json}\n")

    # 최종 보고
    print(f"\n{'='*60}")
    print(f"완료: {success_count}개 성공 / {parse_failed_count}개 parse_failed / 총 {len(ops)}개")
    print(f"총 파라미터 수: {total_params}개 (평균 {total_params/max(success_count,1):.1f}개/op)")
    print(f"저장 위치: {OUTPUT_PATH}")

    if failed_ops:
        print(f"\n[parse_failed 목록]")
        for apinum, op, reason in failed_ops:
            print(f"  apinum={apinum} {op}: {reason}")

    # 공통 파라미터 탐지 (성공 op 대상)
    param_freq: dict[str, int] = {}
    for _, endpoint_op in ops:
        val = results.get(endpoint_op, {})
        if isinstance(val, list):
            for p in val:
                nm = p.get("name", "")
                if nm:
                    param_freq[nm] = param_freq.get(nm, 0) + 1

    threshold = success_count * 0.8
    common = sorted(
        [(nm, cnt) for nm, cnt in param_freq.items() if cnt >= threshold],
        key=lambda x: -x[1],
    )
    if common:
        print(f"\n[공통 파라미터 (성공 {success_count}개 중 80%+ 등장)]")
        for nm, cnt in common:
            print(f"  {nm}: {cnt}개 ({cnt/success_count*100:.0f}%)")

    # 대표 샘플 3건 출력
    print(f"\n[대표 샘플 3건]")
    sample_ops = [op for _, op in ops[:3]]
    for op in sample_ops:
        val = results.get(op)
        if isinstance(val, list):
            print(f"  {op}: {json.dumps(val, ensure_ascii=False)[:200]}")
        else:
            print(f"  {op}: {val}")


if __name__ == "__main__":
    main()
