# VWorld CLI 설치 스크립트 (Windows PowerShell)
# 사용법: irm https://raw.githubusercontent.com/clazic/vworld/main/install.ps1 | iex
# 파라미터:
#   -Dir <경로>   설치 디렉터리 오버라이드 (기본: $env:LOCALAPPDATA\vworld)

param(
    [string]$Dir = ""
)

$ErrorActionPreference = 'Stop'

$RAW_BASE = "https://raw.githubusercontent.com/clazic/vworld/main/skills/app"
$DEFAULT_DIR = Join-Path $env:LOCALAPPDATA "vworld"
$INSTALL_DIR = if ($Dir -ne "") { $Dir } else { $DEFAULT_DIR }
$CONFIG_SUBDIR = "app"   # 바이너리 옆 app\ 에 config.toml 배치 (CLI 탐색 규칙)
$BINARY_NAME = "vworld.exe"

function Write-Info  { param($msg) Write-Host "[vworld] $msg" -ForegroundColor Cyan }
function Write-Ok    { param($msg) Write-Host "[vworld] $msg" -ForegroundColor Green }
function Write-Warn  { param($msg) Write-Host "[vworld] 경고: $msg" -ForegroundColor Yellow }
function Write-Err   { param($msg) Write-Host "[vworld] 오류: $msg" -ForegroundColor Red; exit 1 }

# ── OS 확인 ───────────────────────────────────────────────────────────────────
if (-not $IsWindows -and $PSVersionTable.PSVersion.Major -ge 6) {
    Write-Err "이 스크립트는 Windows 전용입니다. macOS/Linux 는 install.sh 를 사용하세요."
}

# ── 설치 디렉터리 생성 ────────────────────────────────────────────────────────
$ConfigDir = Join-Path $INSTALL_DIR $CONFIG_SUBDIR
$BinaryPath = Join-Path $INSTALL_DIR $BINARY_NAME
$ConfigPath = Join-Path $ConfigDir "config.toml"

if (-not (Test-Path $INSTALL_DIR)) {
    New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null
}
if (-not (Test-Path $ConfigDir)) {
    New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
}

# ── 바이너리 다운로드 ─────────────────────────────────────────────────────────
$RemoteBinary = "vworld-windows.exe"
$BinaryUrl = "$RAW_BASE/$RemoteBinary"

Write-Info "바이너리 다운로드 중: $RemoteBinary"
try {
    Invoke-WebRequest -Uri $BinaryUrl -OutFile $BinaryPath -UseBasicParsing
} catch {
    Write-Err "바이너리 다운로드 실패. 네트워크 연결을 확인하세요.`n  URL: $BinaryUrl`n  오류: $_"
}

# ── config.toml 템플릿 다운로드 (기존 파일이 있으면 덮어쓰지 않음) ─────────────
$ConfigUrl = "$RAW_BASE/config.toml.example"
if (Test-Path $ConfigPath) {
    Write-Warn "기존 config.toml 발견 — 덮어쓰지 않습니다: $ConfigPath"
} else {
    Write-Info "config.toml 템플릿 다운로드 중"
    try {
        Invoke-WebRequest -Uri $ConfigUrl -OutFile $ConfigPath -UseBasicParsing
    } catch {
        Write-Warn "config.toml 다운로드 실패 (바이너리 설치는 계속 진행됩니다): $_"
    }
}

# ── SmartScreen / 실행정책 안내 ───────────────────────────────────────────────
Write-Info "SmartScreen 경고가 뜨면: '추가 정보' → '실행' 을 클릭하세요."
Write-Info "실행 정책 오류 시: Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser"

# ── PATH 추가 안내 ────────────────────────────────────────────────────────────
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$INSTALL_DIR*") {
    Write-Warn "$INSTALL_DIR 가 PATH 에 없습니다."
    Write-Warn "현재 세션에 추가:"
    Write-Warn "  `$env:Path += `";$INSTALL_DIR`""
    Write-Warn "영구 추가:"
    Write-Warn "  [Environment]::SetEnvironmentVariable('Path', `$env:Path + ';$INSTALL_DIR', 'User')"

    # 현재 세션 PATH 자동 추가
    $env:Path += ";$INSTALL_DIR"
}

# ── 설치 완료 출력 ────────────────────────────────────────────────────────────
Write-Ok "설치 완료: $BinaryPath"
Write-Ok "설정 파일: $ConfigPath"
Write-Host ""

# ── 버전 확인 ─────────────────────────────────────────────────────────────────
try {
    & $BinaryPath --version
} catch {
    Write-Warn "--version 실행 실패. 새 PowerShell 세션을 열고 다시 시도하세요: vworld --version"
}

# ── 키 등록 안내 ──────────────────────────────────────────────────────────────
Write-Host ""
Write-Ok "다음 단계: VWorld 인증키를 등록하세요."
Write-Host "  1) 키 발급: https://www.vworld.kr -> 오픈API -> 인증키 신청"
Write-Host "  2) 키 등록:"
Write-Host "       vworld config add-key <발급받은_KEY> --alias main"
Write-Host "  3) 유효성 확인:"
Write-Host "       vworld config test-keys"
Write-Host ""
Write-Ok "설치가 완료되었습니다. 자세한 사용법: https://github.com/clazic/vworld"
