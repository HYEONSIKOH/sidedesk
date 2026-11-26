import com.android.build.api.artifact.SingleArtifact
import com.android.build.gradle.internal.api.BaseVariantOutputImpl

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// 버전은 저장소 Cargo.toml 의 [workspace.package] version 하나에서 정한다(맥, 윈도우와 같음).
val appVersion = Release.versionName(rootDir.resolve("../Cargo.toml"))

// 릴리스 서명 키는 저장소 밖에 두고 환경변수나 ~/.gradle/gradle.properties 로 받는다. 값은 로그에 찍지 않는다.
val keys = Release.KEY_NAMES.associateWith { System.getenv(it) ?: providers.gradleProperty(it).orNull }
val signing = Release.signing(keys, ci = System.getenv("CI") == "true")
logger.lifecycle("안드로이드 서명: ${signing.name}")

android {
    namespace = "io.github.hyeonsikoh.sidedesk"
    compileSdk = 35
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "io.github.hyeonsikoh.sidedesk"
        minSdk = 29
        targetSdk = 35
        versionCode = Release.versionCode(appVersion)
        versionName = appVersion
        ndk { abiFilters += "arm64-v8a" }
    }

    signingConfigs {
        if (signing == Release.Signing.Release) {
            create("release") {
                val store = file(keys.getValue("SIDEDESK_KEYSTORE")!!).canonicalFile
                val repo = rootDir.resolve("..").canonicalFile
                check(!store.path.startsWith(repo.path + File.separator)) { "키스토어는 저장소 밖에 두세요" }
                check(store.isFile) { "SIDEDESK_KEYSTORE 파일이 없습니다" }
                storeFile = store
                storePassword = keys["SIDEDESK_KEYSTORE_PASSWORD"]
                keyAlias = keys["SIDEDESK_KEY_ALIAS"]
                keyPassword = keys["SIDEDESK_KEY_PASSWORD"]
            }
        }
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
            // 키가 있으면 그 키, CI 에서 키가 없으면 서명 없음, 로컬에서 키가 없으면 디버그 키
            signingConfig = when (signing) {
                Release.Signing.Release -> signingConfigs.getByName("release")
                Release.Signing.DebugKey -> signingConfigs.getByName("debug")
                Release.Signing.Unsigned -> null
            }
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

    // 배포 규칙(buildSrc/Release.kt)도 앱 단위 시험에서 같이 돌린다(buildSrc 시험은 따로 불러야 해서).
    sourceSets["test"].java.srcDir("../buildSrc/src/main/kotlin")

    testOptions {
        unitTests.isIncludeAndroidResources = true
        // 화면 그림 시험이 png 를 남기게 한다. 기준 그림과 비교하지 않는다.
        unitTests.all { it.systemProperty("roborazzi.test.record", "true") }
    }

    // 파일 이름에 버전과 서명 종류를 넣는다: SideDesk-0.1.0.apk, -unsigned, -debugkey, -measure
    applicationVariants.all {
        val type = buildType.name
        outputs.all {
            val output = this as BaseVariantOutputImpl
            when (type) {
                "release" -> output.outputFileName = "SideDesk-$appVersion${signing.suffix}.apk"
                "measure" -> output.outputFileName = "SideDesk-$appVersion-measure.apk"
            }
        }
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
    onVariants(selector().withBuildType("release")) { variant ->
        // 릴리스 권한이 정한 목록(이름과 maxSdkVersion)과 다르면 실패한다. 라이브러리가 몰래 더한 권한을 막는다.
        val manifest = variant.artifacts.get(SingleArtifact.MERGED_MANIFEST)
        val applicationId = variant.applicationId
        tasks.register("checkPermissions") {
            group = "verification"
            inputs.file(manifest)
            doLast {
                val actual = Release.permissionsOf(manifest.get().asFile.readText())
                val problems = Release.permissionProblems(actual, Release.expectedPermissions(applicationId.get()))
                if (problems.isNotEmpty()) throw GradleException(problems.joinToString("\n"))
            }
        }
    }
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

// release.yml 은 assembleRelease 만 불러도 권한 확인이 돈다.
tasks.matching { it.name == "assembleRelease" }.configureEach { dependsOn("checkPermissions") }
