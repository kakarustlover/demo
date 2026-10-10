plugins {
    id("com.android.application")
}

android {
    namespace = "com.example.liquidrustdemo"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.example.liquidrustdemo"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    // libliquid_demo.so is produced by `cargo ndk` into app/src/main/jniLibs (see workflow)
    sourceSets["main"].jniLibs.srcDir("src/main/jniLibs")
}
