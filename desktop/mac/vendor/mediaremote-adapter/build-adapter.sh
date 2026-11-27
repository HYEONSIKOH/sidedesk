#!/bin/bash
# Builds MediaRemoteAdapter.framework into $1 (a folder) and copies the Perl script beside it, without cmake.
# /usr/bin/perl (Apple's, entitled to MediaRemote) loads the framework; the app calls the script.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="$1"
fw="$out/MediaRemoteAdapter.framework"
rm -rf "$fw"; mkdir -p "$fw/Versions/A/Resources"
clang -arch arm64 -arch x86_64 -dynamiclib -fobjc-arc -fvisibility=default -I"$here/include" -I"$here/src" \
  "$here"/src/adapter/*.m "$here/src/private/MediaRemote.m" "$here"/src/utility/*.m \
  -framework Foundation -framework AppKit -framework UniformTypeIdentifiers \
  -install_name @rpath/MediaRemoteAdapter.framework/Versions/A/MediaRemoteAdapter -o "$fw/Versions/A/MediaRemoteAdapter"
cat > "$fw/Versions/A/Resources/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>MediaRemoteAdapter</string>
<key>CFBundleIdentifier</key><string>com.vandenbe.MediaRemoteAdapter</string>
<key>CFBundleName</key><string>MediaRemoteAdapter</string>
<key>CFBundlePackageType</key><string>FMWK</string>
<key>CFBundleShortVersionString</key><string>0.1</string>
<key>CFBundleVersion</key><string>0.1.0</string>
</dict></plist>
PLIST
ln -sfn A "$fw/Versions/Current"
ln -sfn Versions/Current/MediaRemoteAdapter "$fw/MediaRemoteAdapter"
ln -sfn Versions/Current/Resources "$fw/Resources"
codesign --force --deep --sign - "$fw" >/dev/null 2>&1
cp "$here/bin/mediaremote-adapter.pl" "$out/mediaremote-adapter.pl"
