# VWorld 제거 스크립트 (Windows PowerShell)
#
# 사용법:
#   irm https://raw.githubusercontent.com/clazic/vworld/main/scripts/uninstall.ps1 | iex
#     → CLI 바이너리·스킬 제거 (%USERPROFILE%\.vworld\config.toml 인증키는 보존)
#
#   파라미터를 주려면 스크립트블록으로 실행:
#     & ([scriptblock]::Create((irm https://raw.githubusercontent.com/clazic/vworld/main/scripts/uninstall.ps1))) -Purge
#
# 파라미터:
#   -Purge          %USERPROFILE%\.vworld (인증키·설정) 까지 모두 제거
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
# 인증키·설정은 바이너리 옆이 아니라 %USERPROFILE%\.vworld\config.toml 에 저장된다.
$CONFIG_DIR = Join-Path $env:USERPROFILE ".vworld"

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

# 스킬 폴더 삭제 전 LEARNINGS.md(사용자 append 오답노트) 백업 — Purge 시엔 백업하지 않음
function Backup-Learnings {
    param($SkillDir)
    $src = Join-Path $SkillDir "LEARNINGS.md"
    if (-not (Test-Path $src)) { return }
    if ($Purge) { return }
    try {
        New-Item -ItemType Directory -Force -Path $CONFIG_DIR | Out-Null
        $dest = Join-Path $CONFIG_DIR "LEARNINGS.backup.md"
        # user·project 스킬이 모두 있을 때 서로 덮어쓰지 않도록 이미 있으면 타임스탬프 부여
        if (Test-Path $dest) { $dest = Join-Path $CONFIG_DIR ("LEARNINGS.backup." + (Get-Date -Format "yyyyMMddHHmmss") + ".md") }
        Copy-Item -Path $src -Destination $dest -Force
        Write-Ok "LEARNINGS.md 백업: $dest"
    } catch {
        Write-Warn "LEARNINGS.md 백업 실패: $src`n  $_"
    }
}

# 1. CLI 바이너리
Remove-Target (Join-Path $INSTALL_DIR $BINARY_NAME) "CLI 바이너리"

# 2. 스킬 (user / project) — 삭제 전 LEARNINGS.md 백업
$UserSkillDir = Join-Path (Join-Path $env:USERPROFILE ".claude") (Join-Path "skills" $SkillName)
$ProjectSkillDir = Join-Path (Join-Path (Get-Location).Path ".claude") (Join-Path "skills" $SkillName)
Backup-Learnings $UserSkillDir
Backup-Learnings $ProjectSkillDir
Remove-Target $UserSkillDir "user 스킬"
Remove-Target $ProjectSkillDir "project 스킬"

# 3. 인증키·설정 %USERPROFILE%\.vworld (Purge 시에만)
if ($Purge) {
    Remove-Target $CONFIG_DIR "인증키·설정 폴더"
    Write-Info "인증키·설정(%USERPROFILE%\.vworld)까지 모두 제거했습니다 (-Purge)."
} else {
    Write-Info "인증키·설정은 보존했습니다 ($(Join-Path $CONFIG_DIR 'config.toml'))."
    Write-Info "완전 삭제: uninstall.ps1 -Purge"
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
