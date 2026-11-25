package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.AccountsSettings
import mycelium.ui.MyceliumTheme
import mycelium.ui.NodesSettings
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val ThreeNodes = """[
{"id":"n3","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.34.55","update_available":"v0.34.66","online":true,"draining":false,
 "agents":[{"agent":"claude","version":"@agentclientprotocol/claude-agent-acp 0.94.0","available":true,"update_available":"0.108.1"},{"agent":"codex","version":"@agentclientprotocol/codex-acp 2.2.2","available":true,"update_available":"2.7.0"},{"agent":"vibe","available":false}],
 "logins":[{"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"signed_in","windows":[{"name":"five_hour","used_percent":2.0},{"name":"seven_day","used_percent":2.0}]},{"agent":"codex","account":"alex@example.org","method":"device_auth","state":"signed_out"}]},
{"id":"n1","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"v0.34.66","online":true,"draining":false,
 "agents":[{"agent":"claude","version":"@agentclientprotocol/claude-agent-acp 0.108.1","available":true},{"agent":"codex","version":"@agentclientprotocol/codex-acp 2.7.0","available":true},{"agent":"vibe","version":"3.1.3","available":true}],
 "logins":[{"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"signed_in"},{"agent":"claude","account":"alex@example.org","method":"oauth_token","state":"signed_in","windows":[{"name":"five_hour","used_percent":100.0},{"name":"seven_day","used_percent":45.0}]},{"agent":"codex","account":"alex@example.org","method":"device_auth","state":"signed_in"}]},
{"id":"n2","name":"orion","hostname":"orion","os":"linux","arch":"x86_64","version":"v0.34.66","online":false,"last_seen_at":"2026-11-20T10:00:00Z","draining":false,
 "agents":[{"agent":"claude","version":"@agentclientprotocol/claude-agent-acp 0.108.1","available":true},{"agent":"codex","version":"@agentclientprotocol/codex-acp 2.7.0","available":true},{"agent":"vibe","available":false}],
 "logins":[{"agent":"claude","account":"alex@example.org","method":"oauth_token","state":"signed_in"}]}]"""

class AccountsAndNodesTablesTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = SynchronousMockEngine { request ->
            when {
                request.url.encodedPath.endsWith("/nodes") -> respond(ThreeNodes, HttpStatusCode.OK, json)
                else -> respond("[]", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }

        return store
    }

    private fun clippedText(): List<String> {
        val texts = rule.onAllNodes(androidx.compose.ui.test.SemanticsMatcher.keyIsDefined(SemanticsProperties.Text), useUnmergedTree = true).fetchSemanticsNodes()

        return texts.filter { node ->
            val layouts = mutableListOf<TextLayoutResult>()
            node.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action?.invoke(layouts)
            layouts.any { it.multiParagraph.intrinsics.maxIntrinsicWidth > it.size.width + 0.5f || it.didOverflowHeight }
        }.map { it.config[SemanticsProperties.Text].joinToString() }
    }

    private fun showAt(width: Int, table: @Composable (Store) -> Unit) {
        val store = store()

        rule.setContent { MyceliumTheme(true) { Box(Modifier.width(width.dp)) { table(store) } } }
        rule.waitForIdle()
    }

    @Test
    fun theNodesTableTruncatesNoTextOnADesktopWidth() {
        showAt(980) { NodesSettings(it, QuietPlatform) {} }

        assertEquals(emptyList(), clippedText())
    }

    @Test
    fun theNodesTableTruncatesNoTextInANarrowWindowAndScrollsInstead() {
        showAt(420) { NodesSettings(it, QuietPlatform) {} }

        assertEquals(emptyList(), clippedText())
    }

    @Test
    fun theAccountsTableTruncatesNoTextOnADesktopWidth() {
        showAt(980) { AccountsSettings(it) }

        assertEquals(emptyList(), clippedText())
    }

    @Test
    fun theAccountsTableTruncatesNoTextInANarrowWindowAndScrollsInstead() {
        showAt(420) { AccountsSettings(it) }

        assertEquals(emptyList(), clippedText())
    }

    @Test
    fun aNodeWithUpdatesHasOneUpdateAndAnOnlineNodeOffersInstallForAMissingAgent() {
        showAt(980) { NodesSettings(it, QuietPlatform) {} }

        rule.onNodeWithContentDescription("Update willow").assertExists()
        rule.onNodeWithContentDescription("0.34.55, update to 0.34.66").assertExists()
        rule.onNodeWithContentDescription("Install Mistral Vibe on willow").assertExists()
        assertEquals(0, rule.onAllNodesWithContentDescription("Install Mistral Vibe on orion").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("Update atlas").fetchSemanticsNodes().size)
        assertTrue(rule.onAllNodes(isRoot()).fetchSemanticsNodes().isNotEmpty())
    }
}
