import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val signingProperties = Properties().apply {
    val file = rootProject.file("keystore.properties")
    if (file.exists()) file.inputStream().use { load(it) }
}

val hasSigningKey = signingProperties.getProperty("storeFile")?.let { rootProject.file(it).exists() } == true

android {
    namespace = "com.nxslauncher"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.nxslauncher"
        minSdk = 26
        targetSdk = 34
        versionCode = 11
        versionName = "1.0.9"
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    signingConfigs {
        if (hasSigningKey) {
            create("release") {
                storeFile = rootProject.file(signingProperties.getProperty("storeFile"))
                storePassword = signingProperties.getProperty("storePassword")
                keyAlias = signingProperties.getProperty("keyAlias")
                keyPassword = signingProperties.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = if (hasSigningKey) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    // The native library is the app: keep it uncompressed so the loader maps it
    // straight from the APK instead of unpacking a copy at install time.
    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
    }

    bundle {
        abi {
            enableSplit = false
        }
    }
}

dependencies {
    implementation("org.jetbrains.kotlin:kotlin-stdlib:2.0.21")
    // Only for FileProvider: the updater has to hand the downloaded APK to the
    // installer as a content:// URI.
    implementation("androidx.core:core:1.13.1")
}
