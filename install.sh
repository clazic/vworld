#!/usr/bin/env bash
set -euo pipefail

# VWorld CLI 설치 스크립트 (macOS / Linux)
# 사용법: curl -fsSL https://raw.githubusercontent.com/clazic/vworld/main/install.sh | bash
# 환경변수:
#   VWORLD_DIR  — 설치 디렉터리 오버라이드 (기본: ~/.local/bin)

RAW_BASE="https://raw.githubusercontent.com/clazic/vworld/main/skills/app"
DEFAULT_DIR="${HOME}/.local/bin"
INSTALL_DIR="${VWORLD_DIR:-${DEFAULT_DIR}}"
BINARY_NAME="vworld"
CONFIG_SUBDIR="app"  # 바이너리 옆 app/ 에 config.toml 배치 (CLI 탐색 규칙)

# ── 색상 출력 헬퍼 ────────────────────────────────────────────────────────────
_info()  { printf '\033[1;34m[vworld]\033[0m %s\n' "$*"; }
_ok()    { printf '\033[1;32m[vworld]\033[0m %s\n' "$*"; }
_warn()  { printf '\033[1;33m[vworld]\033[0m %s\n' "$*" >&2; }
_err()   { printf '\033[1;31m[vworld]\033[0m 오류: %s\n' "$*" >&2; exit 1; }

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

# ── 설치 디렉터리 생성 ────────────────────────────────────────────────────────
mkdir -p "${INSTALL_DIR}"
mkdir -p "${INSTALL_DIR}/${CONFIG_SUBDIR}"

BINARY_PATH="${INSTALL_DIR}/${BINARY_NAME}"
CONFIG_PATH="${INSTALL_DIR}/${CONFIG_SUBDIR}/config.toml"

# ── 바이너리 다운로드 ─────────────────────────────────────────────────────────
_info "바이너리 다운로드 중: ${REMOTE_BINARY}"
if ! curl -fsSL "${RAW_BASE}/${REMOTE_BINARY}" -o "${BINARY_PATH}"; then
  _err "바이너리 다운로드 실패. 네트워크 연결을 확인하세요: ${RAW_BASE}/${REMOTE_BINARY}"
fi

# ── config.toml 템플릿 다운로드 (기존 파일이 있으면 덮어쓰지 않음) ─────────────
if [ -f "${CONFIG_PATH}" ]; then
  _warn "기존 config.toml 발견 — 덮어쓰지 않습니다: ${CONFIG_PATH}"
else
  _info "config.toml 템플릿 다운로드 중"
  if ! curl -fsSL "${RAW_BASE}/config.toml.example" -o "${CONFIG_PATH}"; then
    _warn "config.toml 다운로드 실패 (바이너리 설치는 계속 진행됩니다)"
  fi
fi

# ── 실행 권한 부여 ────────────────────────────────────────────────────────────
chmod +x "${BINARY_PATH}"

# ── macOS Gatekeeper 격리 속성 제거 ──────────────────────────────────────────
if [ "${OS}" = "Darwin" ]; then
  if command -v xattr >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "${BINARY_PATH}" 2>/dev/null || true
    _info "macOS Gatekeeper 격리 속성 제거 완료"
  fi
fi

# ── PATH 안내 ─────────────────────────────────────────────────────────────────
_ok "설치 완료: ${BINARY_PATH}"
_ok "설정 파일: ${CONFIG_PATH}"

# PATH에 없으면 안내
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*)
    ;;
  *)
    _warn "${INSTALL_DIR} 가 PATH 에 없습니다."
    _warn "아래 줄을 ~/.bashrc 또는 ~/.zshrc 에 추가하세요:"
    _warn ""
    _warn "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    _warn ""
    _warn "적용: source ~/.bashrc  (또는 source ~/.zshrc)"
    ;;
esac

# ── 버전 확인 ─────────────────────────────────────────────────────────────────
echo ""
if "${BINARY_PATH}" --version 2>/dev/null; then
  true
else
  _warn "--version 실행 실패. PATH 추가 후 다시 시도하세요: ${BINARY_PATH} --version"
fi

# ── 키 등록 안내 ──────────────────────────────────────────────────────────────
echo ""
_ok "다음 단계: VWorld 인증키를 등록하세요."
echo "  1) 키 발급: https://www.vworld.kr → 오픈API → 인증키 신청"
echo "  2) 키 등록:"
echo "       ${BINARY_PATH} config add-key <발급받은_KEY> --alias main"
echo "  3) 유효성 확인:"
echo "       ${BINARY_PATH} config test-keys"
echo ""
_ok "설치가 완료되었습니다. 자세한 사용법: https://github.com/clazic/vworld"
