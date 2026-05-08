plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.android)
}

android {
    namespace = "io.rapport.fixture.shared"
    compileSdk = 36
}
