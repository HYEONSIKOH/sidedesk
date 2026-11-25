#!/bin/sh
# 사용: build-ios.sh [debug|release]
set -eu
profile=${1:-debug}
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
[ -d /opt/homebrew/opt/rustup/bin ] && PATH="/opt/homebrew/opt/rustup/bin:$PATH"
export PATH
export IPHONEOS_DEPLOYMENT_TARGET=18.0

flag=
[ "$profile" = release ] && flag=--release
for target in aarch64-apple-ios aarch64-apple-ios-sim; do
    cargo rustc -q -p sidedesk-core --lib --crate-type staticlib --features uniffi --target "$target" $flag
done
env -u IPHONEOS_DEPLOYMENT_TARGET cargo build -q -p sidedesk-core --features uniffi

out=ios/Generated
rm -rf "$out"
mkdir -p "$out/headers"
cargo run -q -p sidedesk-bindgen -- generate --library --no-format --language swift \
    --out-dir "$out" target/debug/libsidedesk_core.dylib
mv "$out/sidedesk_coreFFI.h" "$out/headers/"
mv "$out/sidedesk_coreFFI.modulemap" "$out/headers/module.modulemap"
xcodebuild -create-xcframework \
    -library "target/aarch64-apple-ios/$profile/libsidedesk_core.a" -headers "$out/headers" \
    -library "target/aarch64-apple-ios-sim/$profile/libsidedesk_core.a" -headers "$out/headers" \
    -output "$out/SideDeskCore.xcframework" > /dev/null
rm -r "$out/headers"
