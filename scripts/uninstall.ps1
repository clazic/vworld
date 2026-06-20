# VWorld 제거 스크립트 (Windows PowerShell)
#
# 사용법:
#   irm https://raw.githubusercontent.com/clazic/vworld/main/scripts/uninstall.ps1 | iex
#     → CLI 바이너리·스킬 제거 (config 는 보존)
#
#   파라미터를 주려면 스크립트블록으로 실행:
#     & ([scriptblock]::Create((irm https://raw.githubusercontent.com/clazic/vworld/main/scripts/uninstall.ps1))) -Purge
#
# 파라미터:
#   -Purge          config 까지 모두 제거
#   -Dir <경로>     CLI 바이너리 설치 디렉터리 (기본 $env:LOCALAPPDATA\vworld)

param(
    [switch]$Purge,
    [string]$Dir = ""
)

$ErrorActionPreference = 'Stop'

$SkillName = "vworld"
$BINARY_NAME = "vworld.exe"
$DEFAULT_DIR = Join-Path $env:LOCALAPPDATA "vworld"
$INSTALL_DIR = if ($Dir -ne "") { $Dir } else { $DEFAULT_DIR }
$CONFIG_SUBDIR = "app"

function Write-Info { param($msg) Write-Host "[vworld] $msg" -ForegroundColor Cyan }
function Write-Ok   { param($msg) Write-Host "[vworld] $msg" -ForegroundColor Green }
function Write-Warn { param($msg) Write-Host "[vworld] 경고: $msg" -ForegroundColor Yellow }

# 파일/디렉터리 제거 (없으면 조용히 통과)
function Remove-Target {
    param($Path, $Desc)
    if (Test-Path $Path) {
        try {
            Remove-Item -Path $Path -Recurse -Force
            Write-Ok "제거: $Desc ($Path)"
        } catch {
            Write-Warn "제거 실패: $Path`n  $_"
        }
    }
}

# 1. CLI 바이너리
Remove-Target (Join-Path $INSTALL_DIR $BINARY_NAME) "CLI 바이너리"

# 2. 스킬 (user / project) — 폴더째 삭제되어 내부 app\config.toml 도 함께 제거됨
Remove-Target (Join-Path (Join-Path $env:USERPROFILE ".claude") (Join-Path "skills" $SkillName)) "user 스킬"
Remove-Target (Join-Path (Join-Path (Get-Location).Path ".claude") (Join-Path "skills" $SkillName)) "project 스킬"

# 3. cli 모드 config (purge 시에만)
if ($Purge) {
    Remove-Target (Join-Path (Join-Path $INSTALL_DIR $CONFIG_SUBDIR) "config.toml") "cli config"
    Write-Info "config 까지 모두 제거했습니다 (-Purge)."
} else {
    Write-Info "cli 모드 config.toml 은 보존했습니다. 완전 삭제: uninstall.ps1 -Purge"
}

# 4. User PATH 에서 INSTALL_DIR 제거 (vworld 전용 폴더이므로 안전)
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -and $UserPath -like "*$INSTALL_DIR*") {
    $newPath = ($UserPath -split ';' | Where-Object { $_ -and $_ -ne $INSTALL_DIR }) -join ';'
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    Write-Ok "User PATH 에서 제거: $INSTALL_DIR"
}

# 전용 폴더가 비었으면 정리
if ((Test-Path $INSTALL_DIR) -and -not (Get-ChildItem -Path $INSTALL_DIR -Force -ErrorAction SilentlyContinue)) {
    Remove-Item -Path $INSTALL_DIR -Force -ErrorAction SilentlyContinue
    Write-Ok "빈 폴더 제거: $INSTALL_DIR"
}

Write-Host ""
Write-Ok "VWorld 제거 완료."
