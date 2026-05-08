plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jlleitschuh.gradle.ktlint")
    id("io.gitlab.arturbosch.detekt")
}

android {
    namespace = "io.rapport.fixture"
    compileSdk = 36

    productFlavors {
        create("local")
        create("production")
    }
}
