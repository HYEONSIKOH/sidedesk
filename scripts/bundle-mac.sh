#!/bin/sh
# 개발용: 맥 설정 창을 target/SideDesk.app 으로 묶는다. 배포 파일은 dist-mac.sh.
# 사용: bundle-mac.sh <debug|release>
set -eu
profile=${1:-debug}
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
flag=
[ "$profile" = release ] && flag=--release
cargo build -q -p sidedesk $flag

app="target/SideDesk.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "target/$profile/sidedesk" "$app/Contents/MacOS/SideDesk"
[ "$profile" = release ] && strip -x "$app/Contents/MacOS/SideDesk"
. scripts/lib-mac.sh
write_plist "$app" "$(sidedesk_version "$root")"
copy_icon "$app" "$root"
copy_nowplaying "$app" "$root"
# SIDEDESK_SIGN 에 서명 이름을 주면 그것으로(위치 권한 등은 애드혹 서명이면 거부될 수 있다). 없으면 애드혹.
codesign --force --deep --sign "${SIDEDESK_SIGN:--}" "$app" >/dev/null 2>&1
echo "$app"
