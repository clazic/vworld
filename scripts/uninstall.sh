#!/usr/bin/env bash
set -euo pipefail

# VWorld 제거 스크립트 (macOS / Linux)
#
# 사용법:
#   bash uninstall.sh            # CLI 바이너리·스킬 제거 (config 는 보존)
#   bash uninstall.sh --purge    # config 까지 모두 제거
#   VWORLD_PURGE=1 bash uninstall.sh
#
# 환경변수:
#   VWORLD_DIR    CLI 바이너리 설치 디렉터리 (기본 ~/.local/bin)
#   VWORLD_PURGE  1 이면 config 까지 삭제

SKILL_NAME="vworld"
BINARY_NAME="vworld"
INSTALL_DIR="${VWORLD_DIR:-${HOME}/.local/bin}"
CONFIG_SUBDIR="app"

PURGE="${VWORLD_PURGE:-}"
for arg in "$@"; do
  case "$arg" in
    --purge) PURGE=1 ;;
    *) ;;
  esac
done

_info()  { printf '\033[1;34m[vworld]\033[0m %s\n' "$*"; }
_ok()    { printf '\033[1;32m[vworld]\033[0m %s\n' "$*"; }
_warn()  { printf '\033[1;33m[vworld]\033[0m %s\n' "$*" >&2; }

# 파일/디렉터리/심링크 제거 (없으면 조용히 통과)
remove_path() {
  local p="$1" desc="$2"
  if [ -e "$p" ] || [ -L "$p" ]; then
    if rm -rf "$p"; then
      _ok "제거: ${desc} (${p})"
    else
      _warn "제거 실패: ${p}"
    fi
  fi
}

# 1. CLI 바이너리
remove_path "${INSTALL_DIR}/${BINARY_NAME}" "CLI 바이너리"

# 2. 스킬 (user / project) — 스킬 폴더를 통째로 지우면 내부 app/config.toml 도 함께 삭제됨
remove_path "${HOME}/.claude/skills/${SKILL_NAME}" "user 스킬"
remove_path "$(pwd)/.claude/skills/${SKILL_NAME}" "project 스킬"

# 3. cli 모드 config (purge 시에만; 스킬 내부 config 는 위에서 폴더째 삭제됨)
if [ -n "${PURGE}" ]; then
  remove_path "${INSTALL_DIR}/${CONFIG_SUBDIR}/config.toml" "cli config"
  _info "config 까지 모두 제거했습니다 (--purge)."
else
  _info "cli 모드 config.toml 은 보존했습니다 (${INSTALL_DIR}/${CONFIG_SUBDIR}/config.toml)."
  _info "완전 삭제하려면: bash uninstall.sh --purge"
fi

echo ""
_ok "VWorld 제거 완료."
# ~/.local/bin 은 다른 도구와 공유하는 디렉터리이므로 PATH 설정은 건드리지 않는다.
_info "~/.local/bin 의 PATH 설정은 공유 디렉터리라 그대로 두었습니다."
