package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import kotlinx.coroutines.runBlocking
import mycelium.state.BreakClock
import mycelium.state.BreakPhase
import mycelium.state.Store
import mycelium.ui.BreakToast
import mycelium.ui.TimeAndFocusSettings
import mycelium.ui.WellnessScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

class WellnessRenderTest {
    @get:Rule
    val scopes = TestScopes()

    private fun store(): Store {
        val store = Store(WellnessRequests().api(), scopes.create())

        runBlocking { store.wellness.load() }

        return store
    }

    @Test
    fun theWellnessPageSettingsAndToastRenderAtWideAndNarrowWidths() {
        for (dark in listOf(true, false)) {
            for (phone in listOf(false, true)) {
                val store = store()

                assertTrue(renderAt("wellness", dark, phone) { WellnessScreen(store, OpeningPlatform()) }.length() > 3_000)
                assertTrue(renderAt("time-and-focus", dark, phone) { TimeAndFocusSettings(store) }.length() > 3_000)
                assertTrue(renderAt("break-toast", dark, phone) { Box(Modifier.fillMaxSize(), Alignment.BottomEnd) { BreakToast(BreakClock(BreakPhase.Due, activeMinutes = 50), 5, {}, {}, {}) } }.length() > 1_000)
            }
        }
    }
}
