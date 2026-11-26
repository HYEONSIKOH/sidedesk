package io.github.hyeonsikoh.sidedesk

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.remember
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import io.github.hyeonsikoh.sidedesk.desk.DeskActions
import io.github.hyeonsikoh.sidedesk.desk.DeskScreen
import io.github.hyeonsikoh.sidedesk.desk.DeskViewModel
import io.github.hyeonsikoh.sidedesk.desk.Sort
import io.github.hyeonsikoh.sidedesk.link.Topic
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import io.github.hyeonsikoh.sidedesk.home.HomeScreen
import io.github.hyeonsikoh.sidedesk.home.HomeViewModel
import uniffi.sidedesk_core.newRequestId

class MainActivity : ComponentActivity() {
    private val home: HomeViewModel by viewModels {
        viewModelFactory { initializer { HomeViewModel((application as SideDeskApp).link) } }
    }
    private val desk: DeskViewModel by viewModels {
        viewModelFactory {
            initializer { DeskViewModel((application as SideDeskApp).link) { newRequestId().toLong() } }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (savedInstanceState == null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val missing = BLUETOOTH.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
            if (missing.isNotEmpty()) requestPermissions(missing.toTypedArray(), ASK_BLUETOOTH)
        }
        if (BuildConfig.DEBUG && savedInstanceState == null) openForCheck()
        setContent {
            val state = home.state.collectAsStateWithLifecycle().value
            if (state.computerName != null && state.pairingCode == null && state.problem == null) {
                val actions = remember {
                    DeskActions(desk::show, desk::sortBy, desk::ask, desk::cancel, desk::confirmKill, desk::dismissNotice)
                }
                DeskScreen(desk.state.collectAsStateWithLifecycle().value, actions)
            } else {
                HomeScreen(state, home::answer)
            }
        }
    }

    /** 디버그 빌드 전용. 폰을 누르지 않고 화면을 찍어 보려고 am start 값으로 화면을 고른다. */
    private fun openForCheck() {
        if (intent.getStringExtra("screen") == "processes") desk.show(Topic.Processes)
        if (intent.getStringExtra("sort") == "memory") desk.sortBy(Sort.Memory)
        if (intent.getBooleanExtra("ask", false)) {
            lifecycleScope.launch { desk.state.first { it.rows.isNotEmpty() }.rows.first().let(desk::ask) }
        }
    }

    // 화면이 꺼지거나 앱이 뒤로 가면 컴퓨터가 수집을 멈추게 구독을 비운다.
    override fun onStart() {
        super.onStart()
        desk.setVisible(true)
    }

    override fun onStop() {
        desk.setVisible(false)
        super.onStop()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        // 처음 실행에서는 권한을 받기 전에 블루투스를 열려다 실패하므로, 허용됐으면 다시 연다.
        if (requestCode == ASK_BLUETOOTH && grantResults.isNotEmpty() &&
            grantResults.all { it == PackageManager.PERMISSION_GRANTED }
        ) {
            (application as SideDeskApp).link.restartBluetooth()
        }
    }

    private companion object {
        const val ASK_BLUETOOTH = 1
        val BLUETOOTH = listOf(Manifest.permission.BLUETOOTH_ADVERTISE, Manifest.permission.BLUETOOTH_CONNECT)
    }
}
