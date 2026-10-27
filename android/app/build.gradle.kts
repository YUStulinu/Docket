import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

val ndkVersionPinned = "29.0.14206865"
val abis = listOf("arm64-v8a", "x86_64")
val repo = rootProject.projectDir.resolve("..")
val coreDir = repo.resolve("core")
val thirdParty = repo.resolve("native/third_party")

android {
    namespace = "io.github.yustulinu.docket"
    compileSdk = 37
    ndkVersion = ndkVersionPinned

    defaultConfig {
        applicationId = "io.github.yustulinu.docket"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk { abiFilters += abis }
        externalNativeBuild {
            cmake {
                arguments += listOf("-DANDROID_STL=c++_static", "-DCMAKE_BUILD_TYPE=Release")
                cppFlags += listOf("-O2")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Signed with the debug key so that the release build can be installed for testing;
            // a real release is signed with your own key.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    externalNativeBuild {
        cmake {
            path = repo.resolve("native/CMakeLists.txt")
            version = "3.31.6"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { compose = true }

    sourceSets["main"].apply {
        kotlin.directories += layout.buildDirectory.dir("generated/uniffi").get().asFile.path
        jniLibs.directories += layout.buildDirectory.dir("rust/jniLibs").get().asFile.path
        assets.directories += layout.buildDirectory.dir("generated/ocr-assets").get().asFile.path
    }

    packaging {
        // The traineddata files are read as they are (Tesseract maps them); do not compress.
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
    androidResources { noCompress += "traineddata" }
}


// --- the Rust core and its Kotlin bindings --------------------------------------------------------

fun ndkDir(): File {
    System.getenv("ANDROID_NDK_HOME")?.let { return File(it) }
    val props = Properties()
    rootProject.file("local.properties").takeIf { it.exists() }?.inputStream()?.use { props.load(it) }
    val sdk = props.getProperty("sdk.dir") ?: System.getenv("ANDROID_HOME") ?: System.getenv("ANDROID_SDK_ROOT")
        ?: error("set sdk.dir in android/local.properties, or ANDROID_HOME")
    return File(sdk, "ndk/$ndkVersionPinned")
}

val rustOut = layout.buildDirectory.dir("rust/jniLibs")
val bindingsOut = layout.buildDirectory.dir("generated/uniffi")

val cargoNdk by tasks.registering(Exec::class) {
    description = "Builds the Rust core (core/) for Android with cargo-ndk."
    workingDir = coreDir
    inputs.dir(coreDir.resolve("src"))
    inputs.file(coreDir.resolve("Cargo.toml"))
    inputs.file(coreDir.resolve("Cargo.lock"))
    outputs.dir(rustOut)
    doFirst { environment("ANDROID_NDK_HOME", ndkDir().absolutePath) }
    commandLine(
        listOf("cargo", "ndk") + abis.flatMap { listOf("-t", it) } +
            listOf("-P", "26", "-o", rustOut.get().asFile.absolutePath, "build", "--release", "--lib"),
    )
}

// The bindings come from a host build of the core: the Android libraries are stripped, metadata
// included, and the API is the same.
val hostLibrary: String = run {
    val os = System.getProperty("os.name").lowercase()
    val name = when {
        os.contains("win") -> "docket_core.dll"
        os.contains("mac") -> "libdocket_core.dylib"
        else -> "libdocket_core.so"
    }
    coreDir.resolve("target/debug/$name").absolutePath
}

val cargoHost by tasks.registering(Exec::class) {
    workingDir = coreDir
    inputs.dir(coreDir.resolve("src"))
    outputs.file(hostLibrary)
    commandLine("cargo", "build", "--quiet", "--lib")
}

val uniffiBindings by tasks.registering(Exec::class) {
    description = "Generates the Kotlin bindings of the Rust core with UniFFI."
    dependsOn(cargoNdk, cargoHost)
    workingDir = coreDir
    inputs.file(hostLibrary)
    outputs.dir(bindingsOut)
    commandLine(
        "cargo", "run", "--quiet", "--features", "bindgen", "--bin", "uniffi-bindgen", "--",
        "generate", "--library", hostLibrary,
        "--language", "kotlin", "--no-format", "--out-dir", bindingsOut.get().asFile.absolutePath,
    )
}

// Tesseract's Romanian and English models, shipped in the APK (6.5 MB) and copied out on first run.
val ocrAssets by tasks.registering(Copy::class) {
    from(thirdParty.resolve("tessdata")) { include("ron.traineddata", "eng.traineddata") }
    into(layout.buildDirectory.dir("generated/ocr-assets/tessdata"))
}

tasks.named("preBuild") { dependsOn(uniffiBindings, ocrAssets) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.navigation.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons)
    implementation(libs.camera.camera2)
    implementation(libs.camera.lifecycle)
    implementation(libs.camera.view)
    implementation(libs.work.runtime)
    implementation(libs.kotlinx.coroutines.android)
    implementation("${libs.jna.get()}@aar")
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.test.junit)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
}
