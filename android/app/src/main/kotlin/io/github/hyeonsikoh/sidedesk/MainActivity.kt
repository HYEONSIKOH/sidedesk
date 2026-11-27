package io.github.hyeonsikoh.sidedesk

import android.content.Intent
import android.os.SystemClock
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
import io.github.hyeonsikoh.sidedesk.clock.ClockViewModel
import io.github.hyeonsikoh.sidedesk.desk.AiPart
import io.github.hyeonsikoh.sidedesk.desk.ShortcutPart
import io.github.hyeonsikoh.sidedesk.desk.MusicPart
import io.github.hyeonsikoh.sidedesk.rest.BatteryWatch
import io.github.hyeonsikoh.sidedesk.rest.Hold
import io.github.hyeonsikoh.sidedesk.rest.RestPanel
import io.github.hyeonsikoh.sidedesk.rest.RestScreen
import io.github.hyeonsikoh.sidedesk.rest.RestViewModel
import io.github.hyeonsikoh.sidedesk.rest.applyWindow
import io.github.hyeonsikoh.sidedesk.rest.keepScreenOn
import io.github.hyeonsikoh.sidedesk.rest.stopResting
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import io.github.hyeonsikoh.sidedesk.music.MusicActions
import io.github.hyeonsikoh.sidedesk.music.MusicViewModel
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
        viewModelFactory { initializer { AiViewModel(app.link, buzzer) } }
    }
    private val shortcuts: ShortcutViewModel by viewModels {
        viewModelFactory { initializer { ShortcutViewModel(app.link, buzzer) { newRequestId().toLong() } } }
    }
    private val music: MusicViewModel by viewModels {
        viewModelFactory { initializer { MusicViewModel(app.link, buzzer, { newRequestId().toLong() }) } }
    }
    private val clock: ClockViewModel by viewModels {
        viewModelFactory { initializer { ClockViewModel(app.link, settings) } }
    }
    private val rest: RestViewModel by viewModels {
        viewModelFactory { initializer { RestViewModel(settings, ai.state) } }
    }

    /** 세 화면(AI, 단축키, 음악)이 함께 쓰는 진동. 세기와 끔은 쉬기 설정을 따른다. */
    private val buzzer get() = app.buzzer
    private val battery by lazy { BatteryWatch(this) { charging, percent -> power = charging to percent; fitWindow() } }
    /** 이 창에 마지막으로 건 (쉼, 켜 두기). 바뀔 때만 다시 건다. 창마다 새로 시작한다. */
    private var applied: Pair<Boolean, Boolean>? = null
    private var power: Pair<Boolean, Int?> = false to null

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
        if (BuildConfig.CHECK && savedInstanceState == null) openForCheck()
        // 쉼은 Compose 를 거치지 않고 바로 넘긴다. Compose 의 모으기는 화면이 안 보이면 멈추기 때문이다.
        lifecycleScope.launch {
            rest.state.collect {
                desk.setResting(it.resting)
                fitWindow()
            }
        }
        lifecycleScope.launch { clock.state.collect { rest.hold(Hold.Picking, it.picking) } }
        lifecycleScope.launch { desk.state.collect { rest.hold(Hold.Confirm, it.asking != null || askLocation.value) } }
        lifecycleScope.launch {
            home.state.collect { rest.hold(Hold.Pairing, it.pairingCode != null || it.problem != null) }
        }
        fitWindow()
        val models = Models(
            home, desk, ai, shortcuts, music, clock, rest, buzzer.canSetStrength,
            System::currentTimeMillis, SystemClock::elapsedRealtime,
            askLocation, LOCATION_NOTICE, ::answerLocation,
        )
        setContent {
            val theme = clock.state.collectAsStateWithLifecycle().value.theme
            LaunchedEffect(theme) {
                barsFor(theme)
                fitWindow(force = true)
            }
            LaunchedEffect(askLocation.value) { rest.hold(Hold.Confirm, askLocation.value || desk.state.value.asking != null) }
            SideDeskScreen(models)
        }
    }

    /** 쉼과 켜 두기 설정, 배터리로 창(켜 두기, 밝기, 바 숨김)을 다시 건다. */
    private fun fitWindow(force: Boolean = false) {
        val now = rest.state.value
        val want = now.resting to keepScreenOn(now.keepOn, power.first, power.second)
        if (!force && want == applied) return
        applied = want
        applyWindow(want.first, want.second)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        // 시스템 권한 창이 앞에 있으면 그 안의 누름을 앱이 못 보므로 쉬지 않는다.
        rest.hold(Hold.NoFocus, !hasFocus)
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

    // 이미 켜진 앱에 am start 를 다시 보내도 화면을 고르게 한다.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (BuildConfig.CHECK) openForCheck()
    }

    /** 디버그 빌드 전용. 폰을 누르지 않고 화면을 찍어 보려고 am start 값으로 화면을 고른다. */
    private fun openForCheck() {
        // 디버그와 측정 빌드에서만. 예: am start -n …/.MainActivity --es screen music --ez rest true
        Screen.entries.firstOrNull { it.name.equals(intent.getStringExtra("screen"), ignoreCase = true) }?.let(desk::show)
        if (intent.getBooleanExtra("rest", false)) rest.restNow()
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
        rest.setVisible(true)
        battery.start()
        location.refresh()
    }

    override fun onStop() {
        desk.setVisible(false)
        // 접고 펴거나 돌려서 다시 만드는 중이면 쉼을 그대로 둔다. 새 창의 onCreate 가 쉬는 밝기와 바 숨김을 다시 건다.
        stopResting(isChangingConfigurations, rest, desk)
        battery.stop()
        fitWindow()
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
