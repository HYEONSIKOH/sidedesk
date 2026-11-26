plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "io.github.hyeonsikoh.sidedesk"
    compileSdk = 35
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "io.github.hyeonsikoh.sidedesk"
        minSdk = 29
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += "arm64-v8a" }
    }

    buildTypes {
        debug {
            buildConfigField("boolean", "CHECK", "true")
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            buildConfigField("boolean", "CHECK", "false")
        }
        // 실제 폰에서 메모리, 프레임, 배터리를 재는 빌드. 릴리스와 같이 줄이고 디버그 키로 서명해 설치할 수 있다.
        // 화면 고르기 값(am start 의 screen, rest)을 읽는다.
        create("measure") {
            initWith(getByName("release"))
            signingConfig = signingConfigs.getByName("debug")
            matchingFallbacks += "release"
            buildConfigField("boolean", "CHECK", "true")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions { jvmTarget = "17" }

    testOptions {
        unitTests.isIncludeAndroidResources = true
        // 화면 그림 시험이 png 를 남기게 한다. 기준 그림과 비교하지 않는다.
        unitTests.all { it.systemProperty("roborazzi.test.record", "true") }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }
}

abstract class BuildCore : Exec() {
    @get:OutputDirectory
    abstract val libraries: DirectoryProperty

    @get:OutputDirectory
    abstract val kotlin: DirectoryProperty

    @get:Input
    abstract val profile: Property<String>

    init {
        outputs.upToDateWhen { false }
    }

    override fun exec() {
        args(profile.get(), libraries.get().asFile.path, kotlin.get().asFile.path)
        super.exec()
    }
}

androidComponents {
    onVariants { variant ->
        val core = tasks.register<BuildCore>("buildCore${variant.name.replaceFirstChar(Char::uppercase)}") {
            executable = rootDir.resolve("../scripts/build-android.sh").path
            // core 는 디버그 아니면 릴리스로 빌드한다(측정 빌드도 릴리스 core).
            profile = if (variant.buildType == "debug") "debug" else "release"
        }
        variant.sources.jniLibs?.addGeneratedSourceDirectory(core, BuildCore::libraries)
        variant.sources.java?.addGeneratedSourceDirectory(core, BuildCore::kotlin)
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-process:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("net.java.dev.jna:jna:5.18.1@aar")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.9.0")
    testImplementation("org.robolectric:robolectric:4.14.1")
    testImplementation("io.github.takahirom.roborazzi:roborazzi:1.32.2")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-compose:1.32.2")
    testImplementation("androidx.compose.ui:ui-test-junit4")
}
