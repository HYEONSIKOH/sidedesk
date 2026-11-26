import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class ReleaseTest {
    @Test
    fun versionCodes() {
        assertEquals(100, Release.versionCode("0.1.0"))
        assertEquals(10203, Release.versionCode("1.2.3"))
        assertEquals(9999, Release.versionCode("0.99.99"))
    }

    @Test
    fun badVersionsFailTheBuild() {
        for (bad in listOf("0.100.0", "0.1.100", "1.2", "1.2.3-beta", "a.b.c", "")) {
            assertThrows(bad, IllegalArgumentException::class.java) { Release.versionCode(bad) }
        }
    }

    @Test
    fun readsTheWorkspaceVersion() {
        val file = File.createTempFile("Cargo", ".toml")
        file.writeText("[workspace]\nmembers = []\n\n[package]\nversion = \"9.9.9\"\n\n[workspace.package]\nedition = \"2021\"\nversion = \"0.1.0\"\n")
        assertEquals("0.1.0", Release.versionName(file))
        file.delete()
    }

    @Test
    fun aMissingWorkspaceVersionFailsTheBuild() {
        val file = File.createTempFile("Cargo", ".toml")
        file.writeText("[package]\nversion = \"9.9.9\"\n")
        assertThrows(IllegalStateException::class.java) { Release.versionName(file) }
        file.delete()
    }

    @Test
    fun keysComeFromTheEnvironmentOrTheUserFileOnly() {
        val user = File.createTempFile("gradle", ".properties")
        user.writeText("SIDEDESK_KEY_ALIAS=from-user\nSIDEDESK_KEYSTORE=/user/k.jks\n")
        val keys = Release.keys(mapOf("SIDEDESK_KEYSTORE" to "/env/k.jks"), user)
        assertEquals("/env/k.jks", keys["SIDEDESK_KEYSTORE"])
        assertEquals("from-user", keys["SIDEDESK_KEY_ALIAS"])
        assertEquals(null, keys["SIDEDESK_KEY_PASSWORD"])
        assertEquals(listOf("SIDEDESK_KEYSTORE_PASSWORD", "SIDEDESK_KEY_PASSWORD"), Release.missingKeys(keys))
        assertEquals(emptyList<String>(), Release.missingKeys(emptyMap()))
        user.delete()
    }

    @Test
    fun keysInTheRepositoryStopTheBuild() {
        assertEquals(listOf("SIDEDESK_KEY_PASSWORD"), Release.keysInRepository("org.gradle.jvmargs=-Xmx2g\n  SIDEDESK_KEY_PASSWORD = secret\n"))
        assertEquals(emptyList<String>(), Release.keysInRepository("android.useAndroidX=true\n"))
    }

    @Test
    fun signingKinds() {
        val all = Release.KEY_NAMES.associateWith { "x" }
        assertEquals(Release.Signing.Release, Release.signing(all, ci = true))
        assertEquals(Release.Signing.Release, Release.signing(all, ci = false))
        assertEquals(Release.Signing.Unsigned, Release.signing(all - "SIDEDESK_KEY_PASSWORD", ci = true))
        assertEquals(Release.Signing.DebugKey, Release.signing(emptyMap(), ci = false))
        assertEquals(Release.Signing.DebugKey, Release.signing(all + ("SIDEDESK_KEYSTORE" to ""), ci = false))
    }

    @Test
    fun permissions() {
        val manifest = """
            <uses-permission android:name="android.permission.BLUETOOTH" android:maxSdkVersion="30" />
            <uses-permission android:name="android.permission.VIBRATE" />
            <uses-permission android:name="android.permission.INTERNET" />
        """
        val actual = Release.permissionsOf(manifest)
        assertEquals(mapOf("android.permission.BLUETOOTH" to 30, "android.permission.VIBRATE" to null, "android.permission.INTERNET" to null), actual)
        val expected = mapOf("android.permission.BLUETOOTH" to 30, "android.permission.VIBRATE" to null)
        assertEquals(listOf("더해진 권한: android.permission.INTERNET"), Release.permissionProblems(actual, expected))
        val loosened = actual - "android.permission.INTERNET" + ("android.permission.BLUETOOTH" to null)
        assertTrue(Release.permissionProblems(loosened, expected).single().startsWith("maxSdkVersion 이 다름"))
        assertEquals(7, Release.expectedPermissions("a.b").size)
    }
}
