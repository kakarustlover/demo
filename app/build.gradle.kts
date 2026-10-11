plugins {
    id("com.android.application")
}

android {
    namespace = "com.example.liquidrustdemo"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.example.liquidrustdemo.v2"
        minSdk = 26
        targetSdk = 36
        versionCode = 2
        versionName = "2.0"
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // libliquid_demo.so is produced by `cargo ndk` into app/src/main/jniLibs (see workflow)
    sourceSets["main"].jniLibs.srcDir("src/main/jniLibs")
}
