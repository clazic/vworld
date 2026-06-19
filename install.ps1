# VWorld 설치 스크립트 (Windows PowerShell)
#
# 사용법:
#   irm https://raw.githubusercontent.com/clazic/vworld/main/install.ps1 | iex
#     → 기본: Claude Code 스킬을 user 범위(%USERPROFILE%\.claude\skills\vworld)로 설치
#
#   파라미터를 주려면 스크립트블록으로 실행:
#     & ([scriptblock]::Create((irm https://raw.githubusercontent.com/clazic/vworld/main/install.ps1))) -Scope project
#
# 파라미터:
#   -Scope <user|project|cli>   설치 범위 (기본 user). cli = 단독 바이너리만.
#   -Playwright                 Playwright MCP 설치 시도 (claude CLI 필요)
#   -Dir <경로>                 cli 모드 바이너리 설치 디렉터리 (기본 $env:LOCALAPPDATA\vworld)
#   -ZipUrl <URL>               스킬 zip URL 오버라이드 (기본: GitHub Releases latest)

param(
    [ValidateSet("user", "project", "cli", "")]
    [string]$Scope = "",
    [switch]$Playwright,
    [string]$Dir = "",
    [string]$ZipUrl = ""
)

$ErrorActionPreference = 'Stop'

$Repo = "clazic/vworld"
$RAW_BASE = "https://raw.githubusercontent.com/$Repo/main/skills/app"
if ($ZipUrl -eq "") {
    $ZipUrl = "https://github.com/$Repo/releases/latest/download/vworld-skill.zip"
}
$SkillName = "vworld"
$DEFAULT_DIR = Join-Path $env:LOCALAPPDATA "vworld"
$INSTALL_DIR = if ($Dir -ne "") { $Dir } else { $DEFAULT_DIR }
$CONFIG_SUBDIR = "app"
$BINARY_NAME = "vworld.exe"
$RemoteBinary = "vworld-windows.exe"

function Write-Info  { param($msg) Write-Host "[vworld] $msg" -ForegroundColor Cyan }
function Write-Ok    { param($msg) Write-Host "[vworld] $msg" -ForegroundColor Green }
function Write-Warn  { param($msg) Write-Host "[vworld] 경고: $msg" -ForegroundColor Yellow }
function Write-Err   { param($msg) Write-Host "[vworld] 오류: $msg" -ForegroundColor Red; exit 1 }

# ── OS 확인 (PS6+ 에서만 $IsWindows 존재; PS5.1 은 항상 Windows) ───────────────
if ($PSVersionTable.PSVersion.Major -ge 6 -and -not $IsWindows) {
    Write-Err "이 스크립트는 Windows 전용입니다. macOS/Linux 는 install.sh 를 사용하세요."
}

# ── 설치 범위(scope) 결정 — 파라미터 1차, Read-Host 2차, 기본 user ────────────
if ($Scope -eq "") {
    if (-not [Console]::IsInputRedirected) {
        $answer = Read-Host "설치 범위를 선택하세요 [user/project/cli] (기본 user)"
        if ($answer -ne "") { $Scope = $answer }
    }
    if ($Scope -eq "") { $Scope = "user" }
}
if ($Scope -notin @("user", "project", "cli")) {
    Write-Warn "알 수 없는 -Scope '$Scope' → user 로 진행"
    $Scope = "user"
}

# ── Playwright MCP opt-in 헬퍼 ────────────────────────────────────────────────
function Install-PlaywrightMcp {
    $want = $Playwright.IsPresent
    if (-not $want -and -not [Console]::IsInputRedirected) {
        $ans = Read-Host "Playwright MCP 를 설치할까요? (3D 분석 결과 자동추출용, Claude Code 전용) [y/N]"
        if ($ans -match '^(y|Y|yes|YES|1)$') { $want = $true }
    }
    if (-not $want) { return }

    $claude = Get-Command claude -ErrorAction SilentlyContinue
    if (-not $claude) {
        Write-Warn "claude CLI 가 없어 Playwright MCP 설치를 건너뜁니다 (Claude Code 환경 전용)."
        Write-Warn "Claude Code 설치 후: claude mcp add playwright -- npx ``@playwright/mcp``@latest"
        return
    }
    $existing = & claude mcp list 2>$null | Select-String -Pattern "playwright" -Quiet
    if ($existing) {
        Write-Ok "Playwright MCP 가 이미 설치되어 있습니다 — 건너뜁니다."
        return
    }
    Write-Info "Playwright MCP 설치 중 (claude mcp add)..."
    try {
        & claude mcp add playwright -- npx "@playwright/mcp@latest"
        Write-Ok "Playwright MCP 설치 완료. (브라우저 바이너리는 최초 사용 시 자동 다운로드)"
    } catch {
        Write-Warn "Playwright MCP 설치 실패. 수동: claude mcp add playwright -- npx ``@playwright/mcp``@latest"
    }
}

# ── 스킬 설치 (user / project) ────────────────────────────────────────────────
function Install-Skill {
    param([string]$ScopeArg)

    if ($ScopeArg -eq "user") {
        $target = Join-Path (Join-Path $env:USERPROFILE ".claude") (Join-Path "skills" $SkillName)
    } else {
        $target = Join-Path (Join-Path (Get-Location).Path ".claude") (Join-Path "skills" $SkillName)
    }

    # Expand-Archive 모듈 확인 (PS5.0+)
    if (-not (Get-Command Expand-Archive -ErrorAction SilentlyContinue)) {
        Write-Err "Expand-Archive 를 사용할 수 없습니다 (PowerShell 5.0+ 필요). 단독 CLI 만 원하면 -Scope cli"
    }

    Write-Info "스킬 번들 다운로드: $ZipUrl"
    $tmpZip = Join-Path $env:TEMP ("vworld-skill-" + [guid]::NewGuid().ToString() + ".zip")
    $tmpDir = Join-Path $env:TEMP ("vworld-skill-" + [guid]::NewGuid().ToString())
    try {
        try {
            Invoke-WebRequest -Uri $ZipUrl -OutFile $tmpZip -UseBasicParsing
        } catch {
            Write-Err "스킬 번들 다운로드 실패: $ZipUrl`n  (Releases 자산 게시 여부를 확인하세요. -ZipUrl 로 오버라이드 가능)`n  $_"
        }
        New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null
        Expand-Archive -Path $tmpZip -DestinationPath $tmpDir -Force

        # zip 은 skills\ 루트를 포함 → skills\* 를 target 으로 복사
        $src = $tmpDir
        if (Test-Path (Join-Path $tmpDir "skills")) { $src = Join-Path $tmpDir "skills" }

        New-Item -ItemType Directory -Force -Path $target | Out-Null
        Copy-Item -Path (Join-Path $src "*") -Destination $target -Recurse -Force

        # OS별 바이너리를 표준 이름(app\vworld.exe)으로 정규화
        $remote = Join-Path (Join-Path $target "app") $RemoteBinary
        $canonical = Join-Path (Join-Path $target "app") $BINARY_NAME
        if (Test-Path $remote) {
            Copy-Item -Path $remote -Destination $canonical -Force
        }
    } finally {
        if (Test-Path $tmpZip) { Remove-Item $tmpZip -Force -ErrorAction SilentlyContinue }
        if (Test-Path $tmpDir) { Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue }
    }

    Write-Ok "스킬 설치 완료: $target"
    if ($ScopeArg -eq "project") {
        Write-Info "프로젝트 스킬은 이 디렉터리에서 Claude Code 를 실행할 때만 인식됩니다:"
        Write-Info "  $((Get-Location).Path)"
    }

    $script:SkillBin = Join-Path (Join-Path $target "app") $BINARY_NAME
}

# ── 단독 CLI 바이너리 설치 (cli) ──────────────────────────────────────────────
function Install-Cli {
    $ConfigDir = Join-Path $INSTALL_DIR $CONFIG_SUBDIR
    $BinaryPath = Join-Path $INSTALL_DIR $BINARY_NAME
    $ConfigPath = Join-Path $ConfigDir "config.toml"

    if (-not (Test-Path $INSTALL_DIR)) { New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null }
    if (-not (Test-Path $ConfigDir))   { New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null }

    $BinaryUrl = "$RAW_BASE/$RemoteBinary"
    Write-Info "바이너리 다운로드 중: $RemoteBinary"
    try {
        Invoke-WebRequest -Uri $BinaryUrl -OutFile $BinaryPath -UseBasicParsing
    } catch {
        Write-Err "바이너리 다운로드 실패. 네트워크 연결을 확인하세요.`n  URL: $BinaryUrl`n  오류: $_"
    }

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

    Write-Info "SmartScreen 경고가 뜨면: '추가 정보' → '실행' 을 클릭하세요."
    Write-Info "실행 정책 오류 시: Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser"

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath -notlike "*$INSTALL_DIR*") {
        Write-Warn "$INSTALL_DIR 가 PATH 에 없습니다."
        Write-Warn "영구 추가: [Environment]::SetEnvironmentVariable('Path', `$env:Path + ';$INSTALL_DIR', 'User')"
        $env:Path += ";$INSTALL_DIR"
    }

    Write-Ok "설치 완료: $BinaryPath"
    Write-Ok "설정 파일: $ConfigPath"
    $script:SkillBin = $BinaryPath
}

# ── 실행 ──────────────────────────────────────────────────────────────────────
$script:SkillBin = ""
if ($Scope -eq "cli") {
    Install-Cli
} else {
    Install-Skill -ScopeArg $Scope
}

Install-PlaywrightMcp

# ── 버전 확인 ─────────────────────────────────────────────────────────────────
Write-Host ""
try {
    & $script:SkillBin --version
} catch {
    Write-Warn "--version 실행 실패. 경로를 확인하세요: $($script:SkillBin) --version"
}

# ── 키 등록 안내 ──────────────────────────────────────────────────────────────
Write-Host ""
Write-Ok "다음 단계: VWorld 인증키를 등록하세요."
Write-Host "  1) 키 발급: https://www.vworld.kr -> 오픈API -> 인증키 신청"
Write-Host "  2) 키 등록:"
Write-Host "       $($script:SkillBin) config add-key <발급받은_KEY> --alias main"
Write-Host "  3) 유효성 확인:"
Write-Host "       $($script:SkillBin) config test-keys"
Write-Host ""
Write-Ok "설치가 완료되었습니다. 자세한 사용법: https://github.com/clazic/vworld"
