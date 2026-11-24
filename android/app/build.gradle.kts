plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.fioravante.talk"
    compileSdk = 35
    defaultConfig {
        applicationId = "dev.fioravante.talk"
        minSdk = 28
        targetSdk = 35
        versionCode = 4
        versionName = "0.5.0"
        ndk { abiFilters += "arm64-v8a" }
    }
    buildTypes {
        release {
            isMinifyEnabled = false
            // Sem chave própria ainda: assinado com a de depuração, para instalar direto.
            signingConfig = signingConfigs.getByName("debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    packaging {
        jniLibs {
            useLegacyPackaging = false
            // Do sherpa-onnx só o JNI e o onnxruntime são usados.
            excludes += listOf("**/libsherpa-onnx-c-api.so", "**/libsherpa-onnx-cxx-api.so")
        }
    }
}

dependencies {
    // O motor do Parakeet (sherpa-onnx), baixado pelo deps.sh: 50 MB não vão para o git.
    implementation(files("libs/sherpa-onnx-1.13.8.aar"))
}

val fetchDeps by tasks.registering(Exec::class) {
    workingDir = file("..")
    commandLine("./deps.sh")
}

// O motor em Rust (com o whisper.cpp) entra como .so em src/main/jniLibs.
val buildRust by tasks.registering(Exec::class) {
    workingDir = file("../rust")
    commandLine("./build.sh")
}
tasks.named("preBuild") { dependsOn(buildRust, fetchDeps) }
