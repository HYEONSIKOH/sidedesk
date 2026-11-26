package io.github.hyeonsikoh.sidedesk

import android.Manifest
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import io.github.hyeonsikoh.sidedesk.home.HomeScreen
import io.github.hyeonsikoh.sidedesk.home.HomeViewModel

class MainActivity : ComponentActivity() {
    private val home: HomeViewModel by viewModels {
        viewModelFactory { initializer { HomeViewModel((application as SideDeskApp).link) } }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            requestPermissions(arrayOf(Manifest.permission.BLUETOOTH_ADVERTISE, Manifest.permission.BLUETOOTH_CONNECT), 1)
        }
        setContent {
            val state = home.state.collectAsStateWithLifecycle()
            HomeScreen(state.value, home::answer)
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        // 처음 실행에서는 권한을 받기 전에 블루투스를 열려다 실패하므로 다시 연다.
        (application as SideDeskApp).link.restartBluetooth()
    }
}
