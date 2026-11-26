package io.github.hyeonsikoh.sidedesk.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import io.github.hyeonsikoh.sidedesk.R

@Composable
fun HomeScreen(state: HomeState, onAnswer: (Boolean) -> Unit) {
    MaterialTheme(colorScheme = darkColorScheme()) {
        Surface(Modifier.fillMaxSize()) {
            Box(Modifier.padding(24.dp), contentAlignment = Alignment.Center) {
                val code = state.pairingCode
                if (code != null) {
                    Pairing(code, onAnswer)
                } else {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(
                            text = state.problem ?: state.computerName ?: stringResource(R.string.home_waiting),
                            style = MaterialTheme.typography.headlineMedium,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.alpha(if (state.dimmed) 0.4f else 1f),
                        )
                        if (state.dimmed) {
                            Text(
                                text = stringResource(R.string.home_lost),
                                style = MaterialTheme.typography.bodyMedium,
                                modifier = Modifier.padding(top = 8.dp),
                            )
                        }
                        state.notice?.let {
                            Text(
                                text = it,
                                style = MaterialTheme.typography.bodySmall,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.padding(top = 16.dp).alpha(0.6f),
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Pairing(code: String, onAnswer: (Boolean) -> Unit) {
    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Text(stringResource(R.string.pairing_question), style = MaterialTheme.typography.titleMedium)
        Text(code.chunked(3).joinToString(" "), style = MaterialTheme.typography.displayMedium)
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedButton(onClick = { onAnswer(false) }) { Text(stringResource(R.string.pairing_no)) }
            Button(onClick = { onAnswer(true) }) { Text(stringResource(R.string.pairing_yes)) }
        }
    }
}
