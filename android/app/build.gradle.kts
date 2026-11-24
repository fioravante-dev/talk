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
        versionCode = 1
        versionName = "0.3.0"
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
    packaging { jniLibs { useLegacyPackaging = false } }
}

// O motor em Rust (com o whisper.cpp) entra como .so em src/main/jniLibs.
val buildRust by tasks.registering(Exec::class) {
    workingDir = file("../rust")
    commandLine("./build.sh")
}
tasks.named("preBuild") { dependsOn(buildRust) }
