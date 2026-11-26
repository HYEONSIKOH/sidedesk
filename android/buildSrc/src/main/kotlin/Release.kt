import java.io.File

/** 배포 빌드에 쓰는 규칙(버전, 권한, 서명). 앱 gradle 이 부르고 buildSrc 시험이 확인한다. */
object Release {
    /** 저장소 Cargo.toml 의 [workspace.package] version. 맥, 윈도우와 같은 버전을 한 곳에서 정한다. */
    fun versionName(cargoToml: File): String {
        var inPackage = false
        for (raw in cargoToml.readLines()) {
            val line = raw.trim()
            if (line.startsWith("[")) inPackage = line == "[workspace.package]"
            else if (inPackage && line.startsWith("version")) {
                return line.substringAfter("=").trim().trim('"')
            }
        }
        error("${cargoToml.path} 에 [workspace.package] version 이 없습니다")
    }

    /** 주 × 10000 + 부 × 100 + 수. 부와 수는 0~99 라야 겹치지 않는다. */
    fun versionCode(name: String): Int {
        val parts = name.split(".")
        require(parts.size == 3 && parts.all { it.isNotEmpty() && it.all(Char::isDigit) }) { "버전 $name 은 주.부.수 숫자여야 합니다" }
        val (major, minor, patch) = parts.map(String::toInt)
        require(minor < 100 && patch < 100) { "버전 $name: 부와 수는 99 를 넘을 수 없습니다(버전 번호가 겹침)" }
        require(major in 0..20_000) { "버전 $name: 주 버전이 너무 큽니다" }
        return major * 10_000 + minor * 100 + patch
    }

    /** 서명 종류. 키 넷이 있으면 release, 없으면 CI 는 서명 없음, 로컬은 디버그 키 */
    enum class Signing(val suffix: String) { Release(""), Unsigned("-unsigned"), DebugKey("-debugkey") }

    fun signing(keys: Map<String, String?>, ci: Boolean): Signing = when {
        KEY_NAMES.all { !keys[it].isNullOrBlank() } -> Signing.Release
        ci -> Signing.Unsigned
        else -> Signing.DebugKey
    }

    val KEY_NAMES = listOf("SIDEDESK_KEYSTORE", "SIDEDESK_KEYSTORE_PASSWORD", "SIDEDESK_KEY_ALIAS", "SIDEDESK_KEY_PASSWORD")

    /**
     * 키 값은 환경변수나 사용자 홈의 ~/.gradle/gradle.properties 에서만 읽는다.
     * 저장소의 gradle.properties 나 명령 줄 -P 는 보지 않는다(비밀번호가 커밋되거나 셸 기록에 남지 않게).
     */
    fun keys(env: Map<String, String>, userProperties: File?): Map<String, String?> {
        val user = java.util.Properties()
        if (userProperties != null && userProperties.isFile) userProperties.inputStream().use(user::load)
        return KEY_NAMES.associateWith { env[it] ?: user.getProperty(it) }
    }

    /** 저장소 gradle.properties 에 키 값 줄이 있으면 그 이름들. 있으면 빌드를 멈춘다. */
    fun keysInRepository(repoProperties: String): List<String> =
        repoProperties.lines().map { it.trim() }.filter { line -> KEY_NAMES.any { line.startsWith(it) } }.map { it.substringBefore("=").trim() }

    /** 키 넷 중 일부만 있으면 빠진 이름들(오타를 빨리 알리려고). 하나도 없거나 다 있으면 빈 목록 */
    fun missingKeys(keys: Map<String, String?>): List<String> {
        val missing = KEY_NAMES.filter { keys[it].isNullOrBlank() }
        return if (missing.size == KEY_NAMES.size) emptyList() else missing
    }

    /** 릴리스가 쓰는 권한(이름 → maxSdkVersion, 없으면 null). 라이브러리가 더하거나 maxSdk 를 풀면 빌드가 실패한다. */
    fun expectedPermissions(applicationId: String): Map<String, Int?> = mapOf(
        "android.permission.BLUETOOTH" to 30,
        "android.permission.BLUETOOTH_ADMIN" to 30,
        "android.permission.BLUETOOTH_ADVERTISE" to null,
        "android.permission.BLUETOOTH_CONNECT" to null,
        "android.permission.ACCESS_COARSE_LOCATION" to null,
        "android.permission.VIBRATE" to null,
        "$applicationId.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION" to null,
    )

    /** 합친 manifest 의 uses-permission(이름 → maxSdkVersion) */
    fun permissionsOf(manifest: String): Map<String, Int?> =
        Regex("""<uses-permission\b([^>]*)>""").findAll(manifest).associate { tag ->
            val attrs = tag.groupValues[1]
            val name = Regex("""android:name="([^"]+)"""").find(attrs)!!.groupValues[1]
            name to Regex("""android:maxSdkVersion="(\d+)"""").find(attrs)?.groupValues?.get(1)?.toInt()
        }

    /** 틀린 점. 없으면 빈 목록 */
    fun permissionProblems(actual: Map<String, Int?>, expected: Map<String, Int?>): List<String> {
        val problems = mutableListOf<String>()
        (actual.keys - expected.keys).sorted().forEach { problems += "더해진 권한: $it" }
        (expected.keys - actual.keys).sorted().forEach { problems += "빠진 권한: $it" }
        expected.keys.intersect(actual.keys).sorted().forEach {
            if (actual[it] != expected[it]) problems += "maxSdkVersion 이 다름: $it ${actual[it]} (정한 값 ${expected[it]})"
        }
        return problems
    }
}
