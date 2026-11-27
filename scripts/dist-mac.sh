#!/bin/sh
# 맥 배포 파일: 유니버설(애플 칩 + 인텔) SideDesk.app 을 ad-hoc 서명해 .dmg 로 묶고, 비밀값 검사를 돌린다.
# 결과: target/dist/SideDesk-<버전>-mac.dmg
# 사용: dist-mac.sh
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
. scripts/lib-mac.sh
. scripts/release-env.sh
sidedesk_remap "$root"
version=$(sidedesk_version "$root")
# 평소 개발 빌드 캐시를 깨지 않게 따로 둔다.
export CARGO_TARGET_DIR="$root/target/dist-build"
# Info.plist 의 LSMinimumSystemVersion 과 맞춘다.
export MACOSX_DEPLOYMENT_TARGET=11.0
unset CARGO_ENCODED_RUSTFLAGS

for target in aarch64-apple-darwin x86_64-apple-darwin; do
    RUSTFLAGS="${SIDEDESK_REMAP# }" CFLAGS="${SIDEDESK_CMAP# }" \
        cargo build -q --release -p sidedesk --target "$target"
done

dist="$root/target/dist"
stage="$dist/stage"
app="$stage/SideDesk.app"
dmg="$dist/SideDesk-$version-mac.dmg"
rm -rf "$stage" "$dmg"
mkdir -p "$app/Contents/MacOS"
lipo -create \
    "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/sidedesk" \
    "$CARGO_TARGET_DIR/x86_64-apple-darwin/release/sidedesk" \
    -output "$app/Contents/MacOS/SideDesk"
strip -x "$app/Contents/MacOS/SideDesk"
# 두 칩이 다 들어 있는지
for arch in arm64 x86_64; do
    lipo "$app/Contents/MacOS/SideDesk" -verify_arch "$arch"
done
write_plist "$app" "$version"
copy_icon "$app" "$root"
copy_nowplaying "$app" "$root"
codesign --force --sign - "$app"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname SideDesk -srcfolder "$stage" -ov -format UDZO "$dmg"
rm -rf "$stage"

set -f
forbid=
for path in $SIDEDESK_FORBID; do
    forbid="$forbid --forbid $path"
done
# shellcheck disable=SC2086
python3 -I scripts/check_secrets.py --artifact "$dmg" $forbid
set +f

echo "$dmg"
