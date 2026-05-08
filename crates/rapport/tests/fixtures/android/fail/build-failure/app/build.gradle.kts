plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "io.rapport.fixture"
    compileSdk = 36

    productFlavors {
        create("local")
        create("production")
    }
}
