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
# RUSTFLAGS 가 있으면 cargo 가 아래 대상별 RUSTFLAGS(16KB 정렬)를 버린다.
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
. scripts/release-env.sh
sidedesk_remap "$root"

ndk="${ANDROID_NDK_HOME:-${ANDROID_HOME:-$HOME/Library/Android/sdk}/ndk/27.2.12479018}"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ndk/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android29-clang"
# 홈 경로 지우기는 정렬 뒤에 붙인다(APK 의 core 에 빌드한 사람 경로가 남지 않게).
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-Clink-arg=-Wl,-z,max-page-size=16384$SIDEDESK_REMAP"
export CFLAGS_aarch64_linux_android="${SIDEDESK_CMAP# }"

flag=
[ "$profile" = release ] && flag=--release
cargo build -q -p sidedesk-core --features uniffi --target aarch64-linux-android $flag
cargo build -q -p sidedesk-core --features uniffi

rm -rf "$libs" "$kotlin"
mkdir -p "$libs/arm64-v8a" "$kotlin"
cp "target/aarch64-linux-android/$profile/libsidedesk_core.so" "$libs/arm64-v8a/"
cargo run -q -p sidedesk-bindgen -- generate --library --no-format --language kotlin \
    --out-dir "$kotlin" target/debug/libsidedesk_core.dylib
