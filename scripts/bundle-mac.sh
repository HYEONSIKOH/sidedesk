#!/bin/sh
# 맥 설정 창을 SideDesk.app 으로 묶는다. 블루투스 권한과 자동화(음악 앱) 권한 문구(Info.plist)가 있어야 묻는 창이 뜬다.
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
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>io.github.hyeonsikoh.sidedesk.desktop</string>
    <key>CFBundleName</key><string>SideDesk</string>
    <key>CFBundleDisplayName</key><string>SideDesk</string>
    <key>CFBundleExecutable</key><string>SideDesk</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSBluetoothAlwaysUsageDescription</key><string>케이블 없이 기기와 연결합니다.</string>
    <key>NSAppleEventsUsageDescription</key><string>음악 앱과 Spotify 의 재생 곡을 읽고 조작합니다.</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$app" >/dev/null 2>&1
echo "$app"
