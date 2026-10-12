#!/usr/bin/env bash
set -euo pipefail

# VWorld 제거 스크립트 (macOS / Linux)
#
# 사용법:
#   bash uninstall.sh            # CLI 바이너리·스킬 제거 (~/.vworld/config.toml 인증키는 보존)
#   bash uninstall.sh --purge    # ~/.vworld (인증키·설정) 까지 모두 제거
#   VWORLD_PURGE=1 bash uninstall.sh
#
# 환경변수:
#   VWORLD_DIR    CLI 바이너리 설치 디렉터리 (기본 ~/.local/bin)
#   VWORLD_PURGE  1 이면 ~/.vworld (config.toml 등) 까지 삭제

SKILL_NAME="vworld"
BINARY_NAME="vworld"
INSTALL_DIR="${VWORLD_DIR:-${HOME}/.local/bin}"
# 인증키·설정은 바이너리 옆이 아니라 ~/.vworld/config.toml 에 저장된다.
CONFIG_DIR="${HOME}/.vworld"

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

# 스킬 폴더 삭제 전 LEARNINGS.md(사용자 append 오답노트) 백업 — purge 시엔 백업하지 않음
backup_learnings() {
  local skill_dir="$1" dest
  [ -f "${skill_dir}/LEARNINGS.md" ] || return 0
  [ -z "${PURGE}" ] || return 0
  mkdir -p "${CONFIG_DIR}" 2>/dev/null || { _warn "백업 폴더 생성 실패: ${CONFIG_DIR}"; return 0; }
  dest="${CONFIG_DIR}/LEARNINGS.backup.md"
  # user·project 스킬이 모두 있을 때 서로 덮어쓰지 않도록 이미 있으면 타임스탬프 부여
  [ ! -e "${dest}" ] || dest="${CONFIG_DIR}/LEARNINGS.backup.$(date +%Y%m%d%H%M%S).md"
  if cp -f "${skill_dir}/LEARNINGS.md" "${dest}"; then
    _ok "LEARNINGS.md 백업: ${dest}"
  else
    _warn "LEARNINGS.md 백업 실패: ${skill_dir}/LEARNINGS.md"
  fi
}

# 1. CLI 바이너리
remove_path "${INSTALL_DIR}/${BINARY_NAME}" "CLI 바이너리"

# 2. 스킬 (user / project) — 삭제 전 LEARNINGS.md 백업
backup_learnings "${HOME}/.claude/skills/${SKILL_NAME}"
backup_learnings "$(pwd)/.claude/skills/${SKILL_NAME}"
remove_path "${HOME}/.claude/skills/${SKILL_NAME}" "user 스킬"
remove_path "$(pwd)/.claude/skills/${SKILL_NAME}" "project 스킬"

# 3. 인증키·설정 ~/.vworld (purge 시에만)
if [ -n "${PURGE}" ]; then
  remove_path "${CONFIG_DIR}" "인증키·설정 폴더"
  _info "인증키·설정(~/.vworld)까지 모두 제거했습니다 (--purge)."
else
  _info "인증키·설정은 보존했습니다 (${CONFIG_DIR}/config.toml)."
  _info "완전 삭제하려면: bash uninstall.sh --purge"
fi

echo ""
_ok "VWorld 제거 완료."
# ~/.local/bin 은 다른 도구와 공유하는 디렉터리이므로 PATH 설정은 건드리지 않는다.
_info "~/.local/bin 의 PATH 설정은 공유 디렉터리라 그대로 두었습니다."
