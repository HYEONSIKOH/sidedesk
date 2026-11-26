package io.github.hyeonsikoh.sidedesk

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
        setContent {
            val state = home.state.collectAsStateWithLifecycle()
            HomeScreen(state.value, home::answer)
        }
    }
}
