package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.runtime.CompositionLocalProvider
import mycelium.ui.Accent
import mycelium.ui.Appearance
import mycelium.ui.LocalAppearance
import mycelium.ui.MyTheme
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.state.Store
import mycelium.ui.InstructionsSettings
import mycelium.ui.SettingsSection
import mycelium.ui.SettingsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

@Composable
private fun Surface(content: @Composable () -> Unit) {
    Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) { content() }
}

@OptIn(InternalComposeUiApi::class)
class SettingsRenderTest {
    @get:Rule
    val scopes = TestScopes()

    private val long = (1..120).joinToString("\\n") { "Rule $it: keep the diff small." }
    private val extra = mapOf(
        "/experiments" to EnabledExperiments,
        "/instructions" to """{"source":null,"revision":"a","layers":[{"layer":"global","name":"Global","path":"AGENTS.md","text":"$long","version":null}]}""",
        "/extensions" to """{"generated_ui":true,"rich_markdown":true,"mycelium_tools":true}""",
        "/model-providers" to """[{"id":"p1","kind":"openai_compatible","name":"Lab models","base_url":"https://models.example.test/v1","models":["small-model","large-model"],"created_at":"2026-10-22T10:00:00Z"}]""",
        "/memories" to """[{"id":"m1","text":"Keep immich ui in step with the backups packages.","scopes":["work"],"created_by":"claude","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}]""",
    )

    private fun store() = Store(fakeApi(extra = extra), scopes.create()).also { runBlocking { it.refresh() } }

    @Test
    fun settingsListMemoriesAndInstructionsRender() {
        val store = store()
        val me = Me("u", "sam@example.com", "Sam")

        assertTrue(renderAt("settings-list", true, true) { Surface { SettingsScreen(store, me, "https://mycelium.test", onSignOut = {}) } }.length() > 5_000)
        assertTrue(renderAt("settings-memories", true, true, tabs = 2) { Surface { SettingsScreen(store, me, "https://mycelium.test", startOn = SettingsSection.Memories, onSignOut = {}) } }.length() > 5_000)
        val appearance = Appearance(true, Accent.Neutral, Accent.entries.filter { it != Accent.System }, {})
        assertTrue(renderAt("settings-appearance", true, false) { CompositionLocalProvider(LocalAppearance provides appearance) { Surface { SettingsScreen(store, me, "https://mycelium.test", startOn = SettingsSection.Appearance, onSignOut = {}) } } }.length() > 5_000)
        assertTrue(renderAt("settings-appearance", true, true) { CompositionLocalProvider(LocalAppearance provides appearance) { Surface { SettingsScreen(store, me, "https://mycelium.test", startOn = SettingsSection.Appearance, onSignOut = {}) } } }.length() > 5_000)
        assertTrue(renderAt("settings-providers", true, false, tabs = 4) { Surface { SettingsScreen(store, me, "https://mycelium.test", startOn = SettingsSection.Accounts, onSignOut = {}) } }.length() > 5_000)
        assertTrue(renderAt("settings-integrations", true, false) { Surface { SettingsScreen(store, me, "https://mycelium.test", startOn = SettingsSection.ChatTools, onSignOut = {}) } }.length() > 5_000)
        assertTrue(renderAt("settings-instructions", true, false) { Surface { InstructionsSettings(store) } }.length() > 5_000)
    }
}
