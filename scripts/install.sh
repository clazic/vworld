#!/usr/bin/env bash
set -euo pipefail

# VWorld 설치 스크립트 (macOS / Linux)
#
# 사용법:
#   curl -fsSL https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.sh | bash
#     → 기본: Claude Code 스킬을 user 범위(~/.claude/skills/vworld)로 설치
#
#   curl -fsSL .../install.sh | VWORLD_SCOPE=project bash
#     → 현재 디렉터리(.claude/skills/vworld)에 프로젝트 스킬로 설치
#       (환경변수는 반드시 파이프 오른쪽 bash 앞에 둔다 — curl 앞에 두면 bash 로 전달되지 않음)
#
#   curl -fsSL .../install.sh | VWORLD_SCOPE=cli bash
#     → 스킬이 아닌 단독 CLI 바이너리만 ~/.local/bin 에 설치(기존 동작)
#
# 환경변수:
#   VWORLD_SCOPE       user(기본) | project | cli
#   VWORLD_VERSION     릴리스 태그 고정 (예: v0.1.0). 비우면 latest
#   VWORLD_PLAYWRIGHT  1/yes 이면 Playwright MCP 설치 시도 (claude CLI 필요)
#   VWORLD_DIR         cli 모드 바이너리 설치 디렉터리 (기본 ~/.local/bin)
#   VWORLD_ZIP_URL     스킬 zip URL 오버라이드 (기본: GitHub Releases latest)
#   VWORLD_API_KEY     설정 시 설치 후 인증키 자동 등록 (비대화형에서도 키 주입)

REPO="clazic/vworld"
# 바이너리·zip 은 모두 GitHub Releases 자산에서 받는다 (git 에 바이너리 미보관, CI 빌드).
if [ -n "${VWORLD_VERSION:-}" ]; then
  REL_BASE="https://github.com/${REPO}/releases/download/${VWORLD_VERSION}"
else
  REL_BASE="https://github.com/${REPO}/releases/latest/download"
fi
ZIP_URL="${VWORLD_ZIP_URL:-${REL_BASE}/vworld-skill.zip}"
SKILL_NAME="vworld"
DEFAULT_DIR="${HOME}/.local/bin"
INSTALL_DIR="${VWORLD_DIR:-${DEFAULT_DIR}}"
BINARY_NAME="vworld"

# ── 색상 출력 헬퍼 ────────────────────────────────────────────────────────────
_info()  { printf '\033[1;34m[vworld]\033[0m %s\n' "$*"; }
_ok()    { printf '\033[1;32m[vworld]\033[0m %s\n' "$*"; }
_warn()  { printf '\033[1;33m[vworld]\033[0m %s\n' "$*" >&2; }
_err()   { printf '\033[1;31m[vworld]\033[0m 오류: %s\n' "$*" >&2; exit 1; }

# ── 무결성 검증 (릴리스의 SHA256SUMS 와 대조) ────────────────────────────────
# SHA256SUMS 형식: "<hex>  <파일명>". 다운로드 자체가 실패하면(구 릴리스) 경고 후 진행.
SUMS_FILE=""
SUMS_TRIED=""
trap '[ -z "${SUMS_FILE}" ] || rm -f "${SUMS_FILE}"' EXIT

_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    return 1
  fi
}

# _verify_sha256 <로컬파일> <릴리스 자산명> — 불일치 시 return 1, 검증 불가 시 경고 후 return 0
_verify_sha256() {
  local file="$1" name="$2" expected actual
  if [ -z "${SUMS_TRIED}" ]; then
    SUMS_TRIED=1
    local tmp
    tmp="$(mktemp -t vworld-sums.XXXXXX)" || tmp=""
    if [ -n "${tmp}" ] && curl -fsSL "${REL_BASE}/SHA256SUMS" -o "${tmp}" 2>/dev/null; then
      SUMS_FILE="${tmp}"
    else
      [ -z "${tmp}" ] || rm -f "${tmp}"
      _warn "SHA256SUMS 를 받지 못해 무결성 검증을 건너뜁니다 (구 릴리스일 수 있음)."
    fi
  fi
  [ -n "${SUMS_FILE}" ] || return 0
  expected="$(awk -v n="${name}" '$2 == n || $2 == "*" n { print tolower($1); exit }' "${SUMS_FILE}")"
  if [ -z "${expected}" ]; then
    _warn "SHA256SUMS 에 ${name} 항목이 없어 검증을 건너뜁니다."
    return 0
  fi
  if ! actual="$(_sha256 "${file}")"; then
    _warn "sha256sum/shasum 이 없어 무결성 검증을 건너뜁니다."
    return 0
  fi
  actual="$(printf '%s' "${actual}" | tr 'A-F' 'a-f')"
  if [ "${expected}" != "${actual}" ]; then
    _warn "SHA256 불일치: ${name} (기대 ${expected} / 실제 ${actual})"
    return 1
  fi
  _info "SHA256 검증 통과: ${name}"
}

# 같은 inode 에 덮어쓰면 macOS 에서 'Killed: 9' 가 나므로, 같은 디렉터리의 숨김 임시파일로
# 복사한 뒤 mv -f 로 원자 교체한다(새 inode).
_atomic_install() {
  local src="$1" dest="$2" tmp
  tmp="$(dirname "${dest}")/.$(basename "${dest}").tmp.$$"
  if cp "${src}" "${tmp}" && chmod +x "${tmp}" && mv -f "${tmp}" "${dest}"; then
    return 0
  fi
  rm -f "${tmp}"
  return 1
}

# /dev/tty 가 실제로 사용 가능한지(존재만으로는 부족 — 비대화형은 노드는 있으나 쓰기 불가)
_tty_usable() { { true >/dev/tty; } 2>/dev/null; }

# ── curl 확인 ─────────────────────────────────────────────────────────────────
if ! command -v curl >/dev/null 2>&1; then
  _err "curl 이 설치되어 있지 않습니다. curl 을 먼저 설치하세요."
fi

# ── OS / 아키텍처 감지 ────────────────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
  Darwin)
    REMOTE_BINARY="vworld-macos"
    ;;
  Linux)
    case "${ARCH}" in
      x86_64)
        REMOTE_BINARY="vworld-linux"
        ;;
      *)
        _err "Linux ${ARCH} 는 사전빌드 바이너리가 없습니다. 소스에서 빌드하세요: https://github.com/clazic/vworld#방법-3--소스-빌드"
        ;;
    esac
    ;;
  *)
    _err "지원하지 않는 OS: ${OS}. macOS 또는 Linux 에서 실행하세요."
    ;;
esac

# ── 설치 범위(scope) 결정 — env 1차, /dev/tty 2차, 기본 user ──────────────────
# curl|bash 는 stdin 이 스크립트로 점유되므로 read 는 /dev/tty 로 직접 터미널을 읽는다.
SCOPE="${VWORLD_SCOPE:-}"
if [ -z "${SCOPE}" ]; then
  if _tty_usable; then
    {
      printf '\n설치 범위를 선택하세요:\n'
      printf '  1) user    (~/.claude/skills/vworld — 모든 프로젝트에서 사용, 기본)\n'
      printf '  2) project (현재 폴더 .claude/skills/vworld)\n'
      printf '  3) cli     (스킬 없이 단독 CLI 바이너리만 ~/.local/bin)\n'
      printf '> '
    } > /dev/tty 2>/dev/null || true
    read SCOPE < /dev/tty 2>/dev/null || SCOPE=""
    case "${SCOPE}" in
      1) SCOPE="user" ;; 2) SCOPE="project" ;; 3) SCOPE="cli" ;;
    esac
  fi
  SCOPE="${SCOPE:-user}"
fi
case "${SCOPE}" in
  user|project|cli) ;;
  *) _warn "알 수 없는 VWORLD_SCOPE='${SCOPE}' → user 로 진행"; SCOPE="user" ;;
esac

# ── Playwright MCP opt-in 헬퍼 ────────────────────────────────────────────────
maybe_install_playwright() {
  local want="${VWORLD_PLAYWRIGHT:-}"
  if [ -z "${want}" ] && _tty_usable; then
    printf 'Playwright MCP 를 설치할까요? (3D 분석 결과 자동추출용, Claude Code 전용) [y/N]: ' > /dev/tty 2>/dev/null || true
    read want < /dev/tty 2>/dev/null || want="n"
  fi
  case "${want}" in
    1|y|Y|yes|YES) ;;
    *) return 0 ;;
  esac

  if ! command -v claude >/dev/null 2>&1; then
    _warn "claude CLI 가 없어 Playwright MCP 설치를 건너뜁니다 (Claude Code 환경 전용)."
    _warn "Claude Code 설치 후: claude mcp add playwright -- npx @playwright/mcp@latest"
    return 0
  fi
  if claude mcp list 2>/dev/null | grep -qi playwright; then
    _ok "Playwright MCP 가 이미 설치되어 있습니다 — 건너뜁니다."
    return 0
  fi
  _info "Playwright MCP 설치 중 (claude mcp add)..."
  if claude mcp add playwright -- npx @playwright/mcp@latest; then
    _ok "Playwright MCP 설치 완료. (브라우저 바이너리는 최초 사용 시 자동 다운로드)"
  else
    _warn "Playwright MCP 설치 실패. 수동 설치: claude mcp add playwright -- npx @playwright/mcp@latest"
  fi
}

# ── ~/.local/bin 으로 CLI 바이너리 등록 (스킬 설치 시에도 터미널에서 vworld 사용) ──
# kosis 신버전 방식: 심링크 대신 실파일 복사 → 스킬을 지워도 CLI 가 살아있고,
# Windows 와 동일한 모델(복사 + PATH)로 크로스플랫폼 분기를 단순화한다.
link_cli_binary() {
  local srcbin="$1"
  [ -f "${srcbin}" ] || { _warn "CLI 등록 건너뜀 — 바이너리 없음: ${srcbin}"; return 0; }
  mkdir -p "${INSTALL_DIR}"
  if _atomic_install "${srcbin}" "${INSTALL_DIR}/${BINARY_NAME}"; then
    _ok "CLI 등록: ${INSTALL_DIR}/${BINARY_NAME} (어디서나 '${BINARY_NAME}' 실행)"
  else
    _warn "CLI 등록 실패 — 수동 복사: cp '${srcbin}' '${INSTALL_DIR}/${BINARY_NAME}'"
    return 0
  fi
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      _warn "${INSTALL_DIR} 가 PATH 에 없습니다. ~/.zshrc(또는 ~/.bashrc)에 추가하세요:"
      _warn "  export PATH=\"${INSTALL_DIR}:\$PATH\""
      ;;
  esac
}

# ── 스킬 설치 (user / project) ────────────────────────────────────────────────
install_skill() {
  local scope="$1" target
  if [ "${scope}" = "user" ]; then
    target="${HOME}/.claude/skills/${SKILL_NAME}"
  else
    target="$(pwd)/.claude/skills/${SKILL_NAME}"
  fi

  if ! command -v unzip >/dev/null 2>&1; then
    _err "unzip 이 필요합니다 (스킬 zip 해제). 설치 후 다시 실행하세요. (단독 CLI만 원하면 VWORLD_SCOPE=cli)"
  fi

  _info "스킬 번들 다운로드: ${ZIP_URL}"
  local tmpzip tmpdir learn_bak
  tmpzip="$(mktemp -t vworld-skill.XXXXXX)" || _err "임시 파일 생성 실패"
  tmpdir="$(mktemp -d -t vworld-skill-d.XXXXXX)" || _err "임시 디렉터리 생성 실패"
  learn_bak="$(mktemp -t vworld-learnings.XXXXXX)" || _err "임시 파일 생성 실패"
  # shellcheck disable=SC2064
  trap "rm -rf '${tmpdir}' '${tmpzip}' '${learn_bak}'" RETURN

  if ! curl -fL "${ZIP_URL}" -o "${tmpzip}"; then
    _err "스킬 번들 다운로드 실패: ${ZIP_URL}\n  (Releases 자산이 게시되어 있는지 확인하세요. VWORLD_ZIP_URL 로 오버라이드 가능)"
  fi
  # 기본 URL(Releases 의 vworld-skill.zip)일 때만 SHA256SUMS 로 검증 (URL 오버라이드는 대상 아님)
  if [ -z "${VWORLD_ZIP_URL:-}" ]; then
    _verify_sha256 "${tmpzip}" "vworld-skill.zip" || _err "스킬 번들 무결성 검증 실패 — 설치를 중단합니다."
  else
    _warn "VWORLD_ZIP_URL 오버라이드 사용 — SHA256 검증을 건너뜁니다."
  fi
  if ! unzip -q -o "${tmpzip}" -d "${tmpdir}"; then
    _err "스킬 번들 압축 해제 실패."
  fi

  # 신 zip 은 루트 평탄 구조(SKILL.md·app/·references/ 가 곧장 루트).
  # 구 zip 의 skills/ 래퍼는 하위호환으로 함께 처리.
  local src="${tmpdir}"
  [ -d "${tmpdir}/skills" ] && src="${tmpdir}/skills"

  # 재설치 시 사용자가 append 한 LEARNINGS.md(오답노트)를 덮어쓰지 않도록 백업 후 복원
  local have_learn=""
  if [ -f "${target}/LEARNINGS.md" ] && cp -f "${target}/LEARNINGS.md" "${learn_bak}"; then
    have_learn=1
  fi

  mkdir -p "${target}"
  # 기존 바이너리는 지우고 새 inode 로 복사 (macOS 'Killed: 9' 방지)
  rm -f "${target}/app/${BINARY_NAME}" "${target}/app/vworld-macos" "${target}/app/vworld-linux" 2>/dev/null || true
  cp -R "${src}/." "${target}/"

  if [ -n "${have_learn}" ]; then
    if cp -f "${learn_bak}" "${target}/LEARNINGS.md"; then
      _info "기존 LEARNINGS.md 를 보존했습니다."
    else
      _warn "LEARNINGS.md 복원 실패 — 백업본: ${learn_bak}"
    fi
  fi

  # config.toml 보존: 기존 키 설정이 있으면 덮어쓰지 않음
  if [ -f "${target}/app/config.toml" ] && [ -f "${target}/app/config.toml.example" ]; then
    : # 번들에 둘 다 있으면 그대로 둠 (config.toml 은 안전 템플릿)
  fi

  # OS별 바이너리를 표준 이름(app/vworld)으로 정규화 — SKILL.md 가 app/vworld 를 호출
  if [ -f "${target}/app/${REMOTE_BINARY}" ]; then
    _atomic_install "${target}/app/${REMOTE_BINARY}" "${target}/app/${BINARY_NAME}" \
      || _err "바이너리 정규화 실패: ${target}/app/${BINARY_NAME}"
  fi

  # 실행 권한 (Windows 에서는 무의미하므로 Unix 에서만)
  chmod +x "${target}/app/${BINARY_NAME}" 2>/dev/null || true
  for b in "${target}/app/vworld-macos" "${target}/app/vworld-linux"; do
    [ -f "${b}" ] && chmod +x "${b}" 2>/dev/null || true
  done

  # macOS Gatekeeper 격리 속성 제거
  if [ "${OS}" = "Darwin" ] && command -v xattr >/dev/null 2>&1; then
    xattr -dr com.apple.quarantine "${target}/app" 2>/dev/null || true
    _info "macOS Gatekeeper 격리 속성 제거 완료"
  fi

  _ok "스킬 설치 완료: ${target}"
  if [ "${scope}" = "project" ]; then
    _info "프로젝트 스킬은 이 디렉터리에서 Claude Code 를 실행할 때만 인식됩니다:"
    _info "  $(pwd)"
  fi

  # 스킬 설치와 별개로 ~/.local/bin/vworld 에 CLI 도 등록 (터미널 어디서나 사용)
  link_cli_binary "${target}/app/${BINARY_NAME}"

  SKILL_BIN="${target}/app/${BINARY_NAME}"
}

# ── 단독 CLI 바이너리 설치 (cli) ──────────────────────────────────────────────
install_cli() {
  mkdir -p "${INSTALL_DIR}"

  local binary_path tmpbin
  binary_path="${INSTALL_DIR}/${BINARY_NAME}"
  # 같은 디렉터리의 숨김 임시파일로 받은 뒤 mv -f 로 원자 교체 (기존 inode 덮어쓰기 방지)
  tmpbin="${INSTALL_DIR}/.${BINARY_NAME}.tmp.$$"

  _info "바이너리 다운로드 중: ${REMOTE_BINARY} (Releases)"
  if ! curl -fL "${REL_BASE}/${REMOTE_BINARY}" -o "${tmpbin}"; then
    rm -f "${tmpbin}"
    _err "바이너리 다운로드 실패. Releases 자산을 확인하세요: ${REL_BASE}/${REMOTE_BINARY}"
  fi
  if ! _verify_sha256 "${tmpbin}" "${REMOTE_BINARY}"; then
    rm -f "${tmpbin}"
    _err "바이너리 무결성 검증 실패 — 설치를 중단합니다."
  fi

  chmod +x "${tmpbin}"

  if [ "${OS}" = "Darwin" ] && command -v xattr >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "${tmpbin}" 2>/dev/null || true
    _info "macOS Gatekeeper 격리 속성 제거 완료"
  fi

  if ! mv -f "${tmpbin}" "${binary_path}"; then
    rm -f "${tmpbin}"
    _err "바이너리 교체 실패: ${binary_path}"
  fi

  _ok "설치 완료: ${binary_path}"
  # 인증키는 바이너리 옆이 아니라 ~/.vworld/config.toml 에 저장된다(config add-key 가 자동 생성).

  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      _warn "${INSTALL_DIR} 가 PATH 에 없습니다."
      _warn "아래 줄을 ~/.bashrc 또는 ~/.zshrc 에 추가하세요:"
      _warn ""
      _warn "  export PATH=\"${INSTALL_DIR}:\$PATH\""
      _warn ""
      _warn "적용: source ~/.bashrc  (또는 source ~/.zshrc)"
      ;;
  esac

  SKILL_BIN="${binary_path}"
}

# ── 실행 ──────────────────────────────────────────────────────────────────────
SKILL_BIN=""
if [ "${SCOPE}" = "cli" ]; then
  install_cli
else
  install_skill "${SCOPE}"
fi

maybe_install_playwright

# ── 버전 확인 ─────────────────────────────────────────────────────────────────
echo ""
if [ -n "${SKILL_BIN}" ] && "${SKILL_BIN}" --version 2>/dev/null; then
  true
else
  _warn "--version 실행 실패. 경로를 확인하세요: ${SKILL_BIN} --version"
fi

# ── 인증키 등록 (env > 대화형 입력 > 안내) ─────────────────────────────────────
# 키는 ~/.vworld/config.toml 에 저장된다(바이너리가 실제로 읽는 경로).
echo ""
_registered=""
if [ -n "${SKILL_BIN}" ] && [ -n "${VWORLD_API_KEY:-}" ]; then
  _info "VWORLD_API_KEY 로 인증키 등록 중..."
  if "${SKILL_BIN}" config add-key "${VWORLD_API_KEY}" --alias main >/dev/null 2>&1; then
    _ok "인증키 등록 완료 (~/.vworld/config.toml)"; _registered=1
  else
    _warn "인증키 등록 실패 — 수동: ${SKILL_BIN} config add-key <KEY> --alias main"
  fi
elif [ -n "${SKILL_BIN}" ] && _tty_usable; then
  _key=""
  {
    printf '\nVWorld 인증키를 지금 등록하시겠어요? (없으면 Enter 로 건너뛰기)\n'
    printf '키 발급: https://www.vworld.kr → 오픈API → 인증키 신청\n'
    printf '인증키 입력: '
  } > /dev/tty 2>/dev/null || true
  read _key < /dev/tty 2>/dev/null || _key=""
  if [ -n "${_key}" ]; then
    if "${SKILL_BIN}" config add-key "${_key}" --alias main >/dev/null 2>&1; then
      _ok "인증키 등록 완료 (~/.vworld/config.toml)"; _registered=1
    else
      _warn "인증키 등록 실패 — 수동: ${SKILL_BIN} config add-key <KEY> --alias main"
    fi
  fi
fi

if [ -z "${_registered}" ]; then
  _ok "다음 단계: VWorld 인증키를 등록하세요."
  echo "  1) 키 발급: https://www.vworld.kr → 오픈API → 인증키 신청"
  echo "  2) 키 등록: ${SKILL_BIN} config add-key <발급받은_KEY> --alias main"
  echo "  3) 유효성 확인: ${SKILL_BIN} config test-keys"
else
  _info "유효성 확인: ${SKILL_BIN} config test-keys"
fi

echo ""
_ok "설치가 완료되었습니다. 자세한 사용법: https://github.com/clazic/vworld"
