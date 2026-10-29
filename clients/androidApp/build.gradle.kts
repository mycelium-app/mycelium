plugins {
    alias(libs.plugins.androidApplication)
    alias(libs.plugins.composeCompiler)
}

val releaseTag: MatchResult? =
    Regex("^v(\\d+)\\.(\\d+)\\.(\\d+)$").find(providers.gradleProperty("appVersion").orNull ?: System.getenv("MYCELIUM_VERSION") ?: "")

val appVersionName: String =
    releaseTag?.let { "${it.groupValues[1]}.${it.groupValues[2]}.${it.groupValues[3]}" } ?: "0.0.0-dev"

val appVersionCode: Int =
    releaseTag?.let { it.groupValues[1].toInt() * 10000 + it.groupValues[2].toInt() * 100 + it.groupValues[3].toInt() } ?: 1

android {
    namespace = "mycelium.android"
    compileSdk = 37
    buildToolsVersion = "37.0.0"

    defaultConfig {
        applicationId = "slop.mycelium"
        minSdk = 26
        targetSdk = 37
        versionCode = appVersionCode
        versionName = appVersionName
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
}

dependencies {
    implementation(project(":shared"))
    implementation(libs.compose.runtime)
    implementation(libs.compose.foundation)
    implementation(libs.compose.ui)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.browser)
    implementation(libs.kotlinx.serialization.json)
}
