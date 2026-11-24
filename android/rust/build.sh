#!/usr/bin/env bash
# Compila o motor (Rust + whisper.cpp) para o celular e põe a .so, com a
# libc++ do NDK que ela pede, em app/src/main/jniLibs. O Gradle chama isto.
set -euo pipefail
cd "$(dirname "$0")"
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}
export CMAKE_TOOLCHAIN_FILE=$PWD/android.toolchain.cmake CMAKE_GENERATOR=Ninja
# Sem isto o ggml sai para o ARMv8 básico, sem as instruções de produto
# escalar e de matriz int8 que fazem o modelo quantizado andar. Todo celular
# com Snapdragon 8 Gen 1 em diante (o Fold5 tem o 8 Gen 2) tem as três.
export GGML_CPU_ARM_ARCH=${GGML_CPU_ARM_ARCH:-armv8.2-a+dotprod+fp16+i8mm}
out=../app/src/main/jniLibs
cargo ndk -t arm64-v8a -P 28 -o "$out" build --release
cp "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so "$out/arm64-v8a/"
