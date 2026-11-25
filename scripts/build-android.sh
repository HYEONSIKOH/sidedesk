#!/bin/sh
# 사용: build-android.sh <debug|release> <라이브러리 폴더> <코틀린 폴더>
set -eu
profile=$1
libs=$2
kotlin=$3
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
[ -d /opt/homebrew/opt/rustup/bin ] && PATH="/opt/homebrew/opt/rustup/bin:$PATH"
export PATH

ndk="${ANDROID_NDK_HOME:-${ANDROID_HOME:-$HOME/Library/Android/sdk}/ndk/27.2.12479018}"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ndk/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android29-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-Clink-arg=-Wl,-z,max-page-size=16384"

flag=
[ "$profile" = release ] && flag=--release
cargo build -q -p sidedesk-core --features uniffi --target aarch64-linux-android $flag
cargo build -q -p sidedesk-core --features uniffi

rm -rf "$libs" "$kotlin"
mkdir -p "$libs/arm64-v8a" "$kotlin"
cp "target/aarch64-linux-android/$profile/libsidedesk_core.so" "$libs/arm64-v8a/"
cargo run -q -p sidedesk-bindgen -- generate --library --no-format --language kotlin \
    --out-dir "$kotlin" target/debug/libsidedesk_core.dylib
