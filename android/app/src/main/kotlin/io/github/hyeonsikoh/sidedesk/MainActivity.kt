package io.github.hyeonsikoh.sidedesk

import android.graphics.Color
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import io.github.hyeonsikoh.sidedesk.ai.AiViewModel
import io.github.hyeonsikoh.sidedesk.ai.PhoneBuzzer
import io.github.hyeonsikoh.sidedesk.clock.ClockViewModel
import io.github.hyeonsikoh.sidedesk.desk.AiPart
import io.github.hyeonsikoh.sidedesk.desk.ShortcutPart
import io.github.hyeonsikoh.sidedesk.shortcut.ShortcutViewModel
import io.github.hyeonsikoh.sidedesk.clock.PickActions
import io.github.hyeonsikoh.sidedesk.clock.PrefsSettings
import io.github.hyeonsikoh.sidedesk.desk.ClockPart
import io.github.hyeonsikoh.sidedesk.desk.DeskActions
import io.github.hyeonsikoh.sidedesk.desk.DeskScreen
import io.github.hyeonsikoh.sidedesk.desk.DeskViewModel
import io.github.hyeonsikoh.sidedesk.desk.Screen
import io.github.hyeonsikoh.sidedesk.desk.Sort
import io.github.hyeonsikoh.sidedesk.home.HomeScreen
import io.github.hyeonsikoh.sidedesk.home.HomeViewModel
import io.github.hyeonsikoh.sidedesk.link.LocationState
import io.github.hyeonsikoh.sidedesk.location.CoarseLocation
import io.github.hyeonsikoh.sidedesk.location.PermissionResults
import io.github.hyeonsikoh.sidedesk.ui.Confirm
import io.github.hyeonsikoh.sidedesk.ui.SideDeskTheme
import io.github.hyeonsikoh.sidedesk.ui.Theme
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import uniffi.sidedesk_core.newRequestId

class MainActivity : ComponentActivity() {
    private val app get() = application as SideDeskApp
    private val settings by lazy { PrefsSettings(this) }
    private val location by lazy { CoarseLocation(this, app.link) }
    private val home: HomeViewModel by viewModels {
        viewModelFactory { initializer { HomeViewModel(app.link) } }
    }
    private val desk: DeskViewModel by viewModels {
        viewModelFactory { initializer { DeskViewModel(app.link) { newRequestId().toLong() } } }
    }
    private val ai: AiViewModel by viewModels {
        viewModelFactory { initializer { AiViewModel(app.link, PhoneBuzzer(app)) } }
    }
    private val shortcuts: ShortcutViewModel by viewModels {
        viewModelFactory { initializer { ShortcutViewModel(app.link, PhoneBuzzer(app)) { newRequestId().toLong() } } }
    }
    private val clock: ClockViewModel by viewModels {
        viewModelFactory { initializer { ClockViewModel(app.link, settings) } }
    }

    /** 위치 권한을 묻기 전에 한 번 보이는 안내 */
    private val askLocation = mutableStateOf(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // 알림은 어느 화면에서든 받아야 하므로 화면과 상관없이 처음부터 만든다.
        ai.state
        // 첫 그림부터 저장된 테마의 상태 표시줄로 그린다.
        barsFor(clock.state.value.theme)
        askLocation.value = savedInstanceState?.getBoolean(ASKING_LOCATION) ?: false
        if (savedInstanceState == null) {
            val missing = PermissionResults.BLUETOOTH.filter { !granted(it) }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && missing.isNotEmpty()) {
                requestPermissions(missing.toTypedArray(), ASK_BLUETOOTH)
            } else {
                offerLocation()
            }
        }
        if (BuildConfig.DEBUG && savedInstanceState == null) openForCheck()
        setContent {
            val clockState = clock.state.collectAsStateWithLifecycle().value
            LaunchedEffect(clockState.theme) { barsFor(clockState.theme) }
            SideDeskTheme(clockState.theme) {
                val state = home.state.collectAsStateWithLifecycle().value
                if (state.pairingCode != null || state.problem != null) {
                    HomeScreen(state, home::answer)
                } else {
                    val actions = remember {
                        DeskActions(
                            desk::show, desk::next, desk::previous, desk::sortBy, desk::ask, desk::cancel,
                            desk::confirmKill, desk::dismissNotice,
                        )
                    }
                    val pick = remember { PickActions(clock::setFace, clock::setTheme, clock::done) }
                    val part = ClockPart(clockState, System::currentTimeMillis, clock::pick, clock::nextFace, clock::previousFace, pick, state.notice)
                    val aiPart = AiPart(ai.state.collectAsStateWithLifecycle().value, ai::selectTab)
                    DeskScreen(
                        desk.state.collectAsStateWithLifecycle().value,
                        part,
                        actions,
                        aiPart,
                        ShortcutPart(shortcuts.state.collectAsStateWithLifecycle().value, shortcuts::press),
                        modal = if (askLocation.value) LOCATION_NOTICE else null,
                        onModal = ::answerLocation,
                    )
                }
            }
        }
    }

    private fun granted(permission: String) = ContextCompat.checkSelfPermission(this, permission) ==
        android.content.pm.PackageManager.PERMISSION_GRANTED

    /** 위치 권한이 없고 아직 묻지 않았으면 안내부터 띄운다. */
    private fun offerLocation() {
        if (!granted(PermissionResults.LOCATION) && !settings.askedLocation) askLocation.value = true
    }

    /** 바깥 누르기나 뒤로 가기(null)는 이번만 닫고, 다음에 켤 때 다시 묻는다. */
    private fun answerLocation(next: Boolean?) {
        askLocation.value = false
        if (next == null) return
        settings.askedLocation = true
        if (next) {
            requestPermissions(arrayOf(PermissionResults.LOCATION), ASK_LOCATION)
        } else {
            app.link.setLocation(LocationState.Off)
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putBoolean(ASKING_LOCATION, askLocation.value)
    }

    /** 상태 표시줄 아이콘 색을 시스템 설정이 아니라 앱 테마에 맞춘다. */
    private fun barsFor(theme: Theme) {
        val style = if (theme == Theme.Light) {
            SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT)
        } else {
            SystemBarStyle.dark(Color.TRANSPARENT)
        }
        enableEdgeToEdge(style, style)
    }

    /** 디버그 빌드 전용. 폰을 누르지 않고 화면을 찍어 보려고 am start 값으로 화면을 고른다. */
    private fun openForCheck() {
        when (intent.getStringExtra("screen")) {
            "processes" -> desk.show(Screen.Processes)
            "perf" -> desk.show(Screen.Perf)
        }
        if (intent.getStringExtra("sort") == "memory") desk.sortBy(Sort.Memory)
        if (intent.getBooleanExtra("ask", false)) {
            lifecycleScope.launch { desk.state.first { it.rows.isNotEmpty() }.rows.first().let(desk::ask) }
        }
    }

    // 화면이 꺼지거나 앱이 뒤로 가면 컴퓨터가 수집을 멈추게 구독을 비운다.
    override fun onStart() {
        super.onStart()
        desk.setVisible(true)
        clock.setVisible(true)
        location.refresh()
    }

    override fun onStop() {
        desk.setVisible(false)
        clock.setVisible(false)
        location.stop()
        clock.done()
        super.onStop()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        val result = PermissionResults.of(permissions, grantResults)
        // 처음 실행에서는 권한을 받기 전에 블루투스를 열려다 실패하므로, 허용됐으면 다시 연다.
        if (result.restartBluetooth) app.link.restartBluetooth()
        when (result.location) {
            true -> location.refresh()
            false -> app.link.setLocation(LocationState.Off)
            null -> Unit
        }
        // 블루투스 권한 창이 닫힌 뒤에 위치 안내를 띄워 시스템 창이 겹치지 않게 한다.
        if (requestCode == ASK_BLUETOOTH) offerLocation()
    }

    private companion object {
        const val ASK_BLUETOOTH = 1
        const val ASK_LOCATION = 2
        const val ASKING_LOCATION = "askingLocation"
        val LOCATION_NOTICE = Confirm(
            title = "날씨에 대략적인 위치를 씁니다",
            description = "대략적인 위치(소수 첫째 자리)를 컴퓨터를 거쳐 날씨 제공처(기상청 또는 Open-Meteo)로 보냅니다",
            confirmLabel = "다음",
            cancelLabel = "사용 안 함",
            glyph = "⌖",
        )
    }
}
