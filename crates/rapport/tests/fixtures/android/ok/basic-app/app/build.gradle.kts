plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.ktlint)
    alias(libs.plugins.detekt)
}

android {
    namespace = "io.rapport.fixture"
    compileSdk = 36

    defaultConfig {
        applicationId = "io.rapport.fixture"
        minSdk = 28
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
    }

    flavorDimensions += "environment"
    productFlavors {
        create("local") {
            dimension = "environment"
        }
        create("production") {
            dimension = "environment"
        }
    }
}
