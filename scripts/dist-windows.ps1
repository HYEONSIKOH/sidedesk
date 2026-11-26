# 윈도우 배포 파일: sidedesk.exe(설정 창)와 sidedesk-cli.exe(pair, run)를 zip 으로 묶고 비밀값 검사를 돌린다.
# 결과: target\dist\SideDesk-<버전>-windows-x64.zip. PDB 는 넣지 않는다.
# 사용: pwsh scripts/dist-windows.ps1  (PowerShell 7. 5.1 은 BOM 없는 한글 스크립트를 깨뜨린다)
#Requires -Version 7
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Check([string]$what) {
    if ($LASTEXITCODE -ne 0) { throw "$what 실패(끝 코드 $LASTEXITCODE)" }
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $root
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$target = 'x86_64-pc-windows-msvc'

# 컴파일러가 실행 파일에 넣는 경로(패닉 위치, 의존 크레이트)에서 빌드한 곳을 지운다.
# 여럿이 맞으면 뒤에 준 것이 이기므로 넓은 사용자 폴더를 맨 앞에 둔다.
$userHome = $env:USERPROFILE
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $userHome '.cargo' }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $userHome '.rustup' }
$pairs = @(
    @($userHome, 'home'),
    @($rustupHome, 'rustup'),
    @((Join-Path $cargoHome 'git\checkouts'), 'git'),
    @((Join-Path $cargoHome 'registry\src'), 'crates'),
    @($root, 'sidedesk')
)
foreach ($pair in $pairs) {
    # RUSTFLAGS 는 빈칸으로 나뉘므로 경로에 빈칸이 있으면 멈춘다.
    if ($pair[0].Contains(' ')) { throw "경로에 빈칸이 있어 경로 바꾸기를 할 수 없습니다: $($pair[1])" }
}
$flags = @($pairs | ForEach-Object { "--remap-path-prefix=$($_[0])=$($_[1])" })
# exe 안의 PDB 경로를 파일 이름만 남긴다(링커가 적으므로 경로 바꾸기가 미치지 않는다).
$flags += '-Clink-arg=/PDBALTPATH:%_PDB%'
# RUSTFLAGS 가 있으면 cargo 가 대상별 변수를 버리므로 지운다.
Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
Remove-Item Env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $flags -join ' '
# 평소 개발 빌드 캐시를 깨지 않게 따로 둔다.
$env:CARGO_TARGET_DIR = Join-Path $root 'target\dist-build'

cargo build --release -p sidedesk --target $target
Check 'cargo build'

$built = Join-Path $env:CARGO_TARGET_DIR "$target\release"
$dist = Join-Path $root 'target\dist'
$stage = Join-Path $dist 'stage-windows'
$zip = Join-Path $dist "SideDesk-$version-windows-x64.zip"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
if (Test-Path $zip) { Remove-Item $zip -Force }
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item (Join-Path $built 'sidedesk.exe') $stage
Copy-Item (Join-Path $built 'sidedesk-cli.exe') $stage
@"
SideDesk $version (윈도우 x64)

sidedesk.exe      설정 창. 창을 닫아도 알림 영역(시스템 트레이)의 아이콘으로 계속 돕니다.
sidedesk-cli.exe  터미널용. pair 는 기기 등록, run 은 창 없이 연결 유지.
                  이 폴더에서 PowerShell 은 .\sidedesk-cli pair, 명령 프롬프트(cmd)는 sidedesk-cli pair

처음 열 때 SmartScreen 창이 뜨면 "추가 정보 > 실행"을 누릅니다(코드 서명을 하지 않았습니다).
설정 창은 WebView2 런타임을 씁니다. 윈도우 10, 11 에는 대개 깔려 있습니다.
"@ | Set-Content -Path (Join-Path $stage 'README.txt') -Encoding utf8
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Remove-Item $stage -Recurse -Force

# 빌드한 곳 경로가 남지 않았는지. 바이너리 안 경로는 역슬래시와 빗금이 섞이므로 두 꼴 다 찾는다.
$forbid = @()
foreach ($path in @($userHome, $cargoHome, $rustupHome, $root)) {
    $forbid += '--forbid', $path, '--forbid', $path.Replace('\', '/')
}
python -I scripts/check_secrets.py --artifact $zip @forbid
Check '비밀값 검사'
Write-Output $zip
