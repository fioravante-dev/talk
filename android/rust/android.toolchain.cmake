# O cmake-rs não passa ANDROID_ABI nem ANDROID_PLATFORM para o whisper.cpp:
# este arquivo fixa os dois e entrega o resto ao toolchain do NDK.
# (O build.rs do whisper-rs-sys repassa as variáveis CMAKE_* do ambiente.)
set(ANDROID_ABI arm64-v8a)
set(ANDROID_PLATFORM android-28)
include($ENV{ANDROID_NDK_HOME}/build/cmake/android.toolchain.cmake)
