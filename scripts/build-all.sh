#!/usr/bin/env bash
# 3-OS 크로스빌드 → skills/app/ 배치.
#  - macOS: arm64 + x64 → lipo universal (vworld-macos)
#  - Windows: x86_64-pc-windows-gnu (mingw 링커, .cargo/config.toml) → vworld-windows.exe
#  - Linux: docker rust 이미지 빌드(호스트 크로스링커 불필요) → vworld-linux
# rustup 툴체인을 강제 우선(Homebrew rust는 네이티브 타깃만 보유).
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT="$(pwd)"
APP="$ROOT/skills/app"
mkdir -p "$APP"
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_HOME="${CARGO_HOME:-/tmp/.cargo-vworld}"
ok(){ echo "✓ $*"; }; warn(){ echo "⚠ $*"; }

echo "== [1/3] macOS (arm64 + x64 universal) =="
cargo build --release --target aarch64-apple-darwin \
 && cargo build --release --target x86_64-apple-darwin
if [ -f target/aarch64-apple-darwin/release/vworld ] && [ -f target/x86_64-apple-darwin/release/vworld ]; then
  lipo -create -output "$APP/vworld-macos" \
    target/aarch64-apple-darwin/release/vworld \
    target/x86_64-apple-darwin/release/vworld
  cp target/aarch64-apple-darwin/release/vworld "$APP/vworld"   # SKILL.md 기본(네이티브)
  ok "vworld-macos (universal), vworld(arm64)"
else warn "macOS 빌드 실패"; fi

echo "== [2/3] Windows (x86_64-pc-windows-gnu) =="
if cargo build --release --target x86_64-pc-windows-gnu; then
  cp target/x86_64-pc-windows-gnu/release/vworld.exe "$APP/vworld-windows.exe" && ok "vworld-windows.exe"
else warn "Windows 빌드 실패(mingw 링커 확인)"; fi

echo "== [3/3] Linux x86_64 (docker amd64 에뮬) =="
# Apple Silicon 호스트: --platform linux/amd64 로 x86_64 컨테이너 에뮬 → 네이티브 빌드(QEMU, 느림).
# OrbStack/Docker 데몬 필요(꺼져 있으면 `orb start`). 산출물은 컨테이너 기본 타깃(target/release).
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  docker run --rm --platform linux/amd64 -v "$ROOT":/app -w /app \
    -e CARGO_HOME=/app/.docker-cargo rust:latest \
    bash -c "cargo build --release" \
  && cp target/release/vworld "$APP/vworld-linux" && ok "vworld-linux (x86_64)"
else warn "docker 데몬 미가동 — Linux 빌드 건너뜀 (orb start 후 재실행)"; fi

echo "== 결과: skills/app/ =="
ls -la "$APP" | grep -E 'vworld' || true
