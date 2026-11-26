# 맥 묶기 공용 조각. source 해서 쓴다.

# 저장소 버전(Cargo.toml workspace)
sidedesk_version() {
    sed -n 's/^version = "\(.*\)"/\1/p' "$1/Cargo.toml" | head -1
}

# write_plist <앱 폴더> <버전>
# 블루투스 권한과 자동화(음악 앱) 권한 문구가 있어야 묻는 창이 뜬다.
write_plist() {
    cat > "$1/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>io.github.hyeonsikoh.sidedesk.desktop</string>
    <key>CFBundleName</key><string>SideDesk</string>
    <key>CFBundleDisplayName</key><string>SideDesk</string>
    <key>CFBundleExecutable</key><string>SideDesk</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$2</string>
    <key>CFBundleVersion</key><string>$2</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSBluetoothAlwaysUsageDescription</key><string>케이블 없이 기기와 연결합니다.</string>
    <key>NSAppleEventsUsageDescription</key><string>음악 앱과 Spotify 의 재생 곡을 읽고 조작합니다.</string>
</dict>
</plist>
PLIST
}
