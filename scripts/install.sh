#!/usr/bin/env bash
set -euo pipefail

# VWorld 설치 스크립트 (macOS / Linux)
#
# 사용법:
#   curl -fsSL https://raw.githubusercontent.com/clazic/vworld/main/scripts/install.sh | bash
#     → 기본: Claude Code 스킬을 user 범위(~/.claude/skills/vworld)로 설치
#
#   VWORLD_SCOPE=project curl -fsSL .../install.sh | bash
#     → 현재 디렉터리(.claude/skills/vworld)에 프로젝트 스킬로 설치
#
#   VWORLD_SCOPE=cli curl -fsSL .../install.sh | bash
#     → 스킬이 아닌 단독 CLI 바이너리만 ~/.local/bin 에 설치(기존 동작)
#
# 환경변수:
#   VWORLD_SCOPE       user(기본) | project | cli
#   VWORLD_VERSION     릴리스 태그 고정 (예: v0.1.0). 비우면 latest
#   VWORLD_PLAYWRIGHT  1/yes 이면 Playwright MCP 설치 시도 (claude CLI 필요)
#   VWORLD_DIR         cli 모드 바이너리 설치 디렉터리 (기본 ~/.local/bin)
#   VWORLD_ZIP_URL     스킬 zip URL 오버라이드 (기본: GitHub Releases latest)

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
CONFIG_SUBDIR="app"  # 바이너리 옆 app/ 에 config.toml 배치 (CLI 탐색 규칙)

# ── 색상 출력 헬퍼 ────────────────────────────────────────────────────────────
_info()  { printf '\033[1;34m[vworld]\033[0m %s\n' "$*"; }
_ok()    { printf '\033[1;32m[vworld]\033[0m %s\n' "$*"; }
_warn()  { printf '\033[1;33m[vworld]\033[0m %s\n' "$*" >&2; }
_err()   { printf '\033[1;31m[vworld]\033[0m 오류: %s\n' "$*" >&2; exit 1; }

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
  if cp -f "${srcbin}" "${INSTALL_DIR}/${BINARY_NAME}"; then
    chmod +x "${INSTALL_DIR}/${BINARY_NAME}" 2>/dev/null || true
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
  local tmpzip tmpdir
  tmpzip="$(mktemp -t vworld-skill.XXXXXX)" || _err "임시 파일 생성 실패"
  tmpdir="$(mktemp -d -t vworld-skill-d.XXXXXX)" || _err "임시 디렉터리 생성 실패"
  # shellcheck disable=SC2064
  trap "rm -rf '${tmpdir}' '${tmpzip}'" RETURN

  if ! curl -fL "${ZIP_URL}" -o "${tmpzip}"; then
    _err "스킬 번들 다운로드 실패: ${ZIP_URL}\n  (Releases 자산이 게시되어 있는지 확인하세요. VWORLD_ZIP_URL 로 오버라이드 가능)"
  fi
  if ! unzip -q -o "${tmpzip}" -d "${tmpdir}"; then
    _err "스킬 번들 압축 해제 실패."
  fi

  # 신 zip 은 루트 평탄 구조(SKILL.md·app/·references/ 가 곧장 루트).
  # 구 zip 의 skills/ 래퍼는 하위호환으로 함께 처리.
  local src="${tmpdir}"
  [ -d "${tmpdir}/skills" ] && src="${tmpdir}/skills"

  mkdir -p "${target}"
  cp -R "${src}/." "${target}/"

  # config.toml 보존: 기존 키 설정이 있으면 덮어쓰지 않음
  if [ -f "${target}/app/config.toml" ] && [ -f "${target}/app/config.toml.example" ]; then
    : # 번들에 둘 다 있으면 그대로 둠 (config.toml 은 안전 템플릿)
  fi

  # OS별 바이너리를 표준 이름(app/vworld)으로 정규화 — SKILL.md 가 app/vworld 를 호출
  if [ -f "${target}/app/${REMOTE_BINARY}" ]; then
    cp -f "${target}/app/${REMOTE_BINARY}" "${target}/app/${BINARY_NAME}"
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
  mkdir -p "${INSTALL_DIR}/${CONFIG_SUBDIR}"

  local binary_path config_path
  binary_path="${INSTALL_DIR}/${BINARY_NAME}"
  config_path="${INSTALL_DIR}/${CONFIG_SUBDIR}/config.toml"

  _info "바이너리 다운로드 중: ${REMOTE_BINARY} (Releases)"
  if ! curl -fL "${REL_BASE}/${REMOTE_BINARY}" -o "${binary_path}"; then
    _err "바이너리 다운로드 실패. Releases 자산을 확인하세요: ${REL_BASE}/${REMOTE_BINARY}"
  fi

  if [ -f "${config_path}" ]; then
    _warn "기존 config.toml 발견 — 덮어쓰지 않습니다: ${config_path}"
  else
    _info "config.toml 템플릿 생성: ${config_path}"
    cat > "${config_path}" <<'EOF'
# VWorld CLI 설정 — 본인의 VWorld OpenAPI 인증키를 입력하세요.
#
# 1) 키 발급: https://www.vworld.kr → 오픈API → 인증키 신청
# 2) 등록(권장): vworld config add-key <발급받은_KEY> --alias main
# 3) 또는 아래 [[keys]] 블록의 주석을 풀고 직접 입력
#
# 도메인 등록 키면 referer에 등록 도메인을 적습니다(무도메인 서버 키는 생략).

# [[keys]]
# key = "여기에-발급받은-인증키"
# referer = "https://your-domain.com"

# [[keys]]
# key = "두-번째-키"
# alias = "key2"
EOF
  fi

  chmod +x "${binary_path}"

  if [ "${OS}" = "Darwin" ] && command -v xattr >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "${binary_path}" 2>/dev/null || true
    _info "macOS Gatekeeper 격리 속성 제거 완료"
  fi

  _ok "설치 완료: ${binary_path}"
  _ok "설정 파일: ${config_path}"

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

# ── 키 등록 안내 ──────────────────────────────────────────────────────────────
echo ""
_ok "다음 단계: VWorld 인증키를 등록하세요."
echo "  1) 키 발급: https://www.vworld.kr → 오픈API → 인증키 신청"
echo "  2) 키 등록:"
echo "       ${SKILL_BIN} config add-key <발급받은_KEY> --alias main"
echo "  3) 유효성 확인:"
echo "       ${SKILL_BIN} config test-keys"
echo ""
_ok "설치가 완료되었습니다. 자세한 사용법: https://github.com/clazic/vworld"
