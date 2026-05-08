plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "io.rapport.fixture.app"
    compileSdk = 36

    flavorDimensions += "environment"
    productFlavors {
        create("local")
        create("production")
    }
}
