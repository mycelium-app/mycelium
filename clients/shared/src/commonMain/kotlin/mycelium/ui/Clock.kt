package mycelium.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import kotlinx.coroutines.delay
import kotlin.time.Clock
import kotlin.time.Instant

@Composable
fun rememberNow(periodMillis: Long, ticking: Boolean = true): Instant {
    val now by produceState(Clock.System.now(), periodMillis, ticking) {
        while (ticking) {
            delay(periodMillis)
            value = Clock.System.now()
        }
    }

    return now
}
