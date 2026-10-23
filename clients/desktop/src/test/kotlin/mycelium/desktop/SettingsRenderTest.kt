package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventType
import mycelium.ui.MyTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.state.Store
import mycelium.ui.InstructionsSettings
import mycelium.ui.SettingsScreen
import kotlin.test.Test
import kotlin.test.assertTrue

@Composable
private fun Surface(content: @Composable () -> Unit) {
    Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) { content() }
}

@OptIn(InternalComposeUiApi::class)
class SettingsRenderTest {
    private val long = (1..120).joinToString("\\n") { "Rule $it: keep the diff small." }
    private val extra = mapOf(
        "/instructions" to """{"source":null,"revision":"a","layers":[{"layer":"global","name":"Global","path":"AGENTS.md","text":"$long","version":null}]}""",
        "/extensions" to """{"generated_ui":true,"rich_markdown":true,"mycelium_tools":true}""",
        "/memories" to """[{"id":"m1","text":"Keep immich ui in step with the backups packages.","scopes":["work"],"created_by":"claude","created_at":"2026-10-22T10:00:00Z"}]""",
    )

    private fun store() = Store(fakeApi(extra = extra), CoroutineScope(Dispatchers.Unconfined)).also { runBlocking { it.refresh() } }

    @Test
    fun settingsListMemoriesAndInstructionsRender() {
        val store = store()
        val me = Me("u", "sam@example.com", "Sam")

        val open: (ImageComposeScene) -> Unit = { scene ->
            scene.sendPointerEvent(PointerEventType.Press, Offset(100f, 265f))
            scene.sendPointerEvent(PointerEventType.Release, Offset(100f, 265f))
            scene.render(32_000_000L)
            scene.render(64_000_000L)
            Unit
        }

        assertTrue(renderAt("settings-list", true, true) { Surface { SettingsScreen(store, me, "https://mycelium.test") {} } }.length() > 5_000)
        assertTrue(renderAt("settings-memories", true, true, interact = open) { Surface { SettingsScreen(store, me, "https://mycelium.test") {} } }.length() > 5_000)
        assertTrue(renderAt("settings-instructions", true, false) { Surface { InstructionsSettings(store) } }.length() > 5_000)
    }
}
