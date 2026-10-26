package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.onNodeWithContentDescription
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import mycelium.api.MyceliumApi
import mycelium.api.parseStreamMessage
import mycelium.state.Store
import mycelium.api.Me
import mycelium.ui.SettingsScreen
import mycelium.ui.SettingsSection
import mycelium.ui.MemoriesSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test

private fun memory(id: String, text: String) =
    """{"id":"$id","text":"$text","scopes":["global"],"created_by":"laptop/Fix login","conversation_id":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

class MemoriesLiveTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun showMemories(stored: List<String>, inSettings: Boolean = false): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val asked = request.url.parameters["query"].orEmpty()
            val matching = if (asked.isEmpty()) stored else stored.filter { asked in it }

            respond("[" + matching.joinToString(",") + "]", HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        rule.setContent {
            MyceliumTheme(true) {
                if (inSettings) {
                    SettingsScreen(store, Me("u", "reader@example.test", "Reader"), "https://mycelium.test", startOn = SettingsSection.Memories, onSignOut = {})
                } else {
                    MemoriesSettings(store)
                }
            }
        }

        return store
    }

    private fun shows(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun stream(store: Store, text: String) = store.receive(parseStreamMessage(text)!!)

    @Test
    fun memoriesOpenInsideSettingsAndScrollToTheLastEntry() {
        showMemories((1..60).map { memory("m$it", "Memory entry $it") }, inSettings = true)
        rule.waitUntil(5_000) { shows("Memory entry 60") }
        rule.onNodeWithText("Memory entry 60").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun aMemoryShowsWhereItCameFromAndLinksToItsConversation() {
        val sourced = memory("m1", "Staging listens on 5433")
            .replace("\"conversation_id\":null", "\"conversation_id\":\"c1\",\"source\":{\"model\":\"claude-opus-5-5\",\"agent\":\"claude\",\"node_name\":\"laptop\",\"project_name\":\"Mycelium\",\"conversation_title\":\"Fix login\"}")
        showMemories(listOf(sourced))
        rule.waitUntil(5_000) { shows("Staging listens on 5433") }
        rule.onNodeWithText("claude-opus-5-5", substring = true).assertExists()
        rule.onNode(hasText("Fix login") and hasClickAction()).assertExists()
    }

    @Test
    fun memoryActionsOpenFromTheRowMenu() {
        showMemories(listOf(memory("m1", "Staging listens on 5433")))
        rule.waitUntil(5_000) { shows("Staging listens on 5433") }
        rule.onNodeWithContentDescription("More for Staging listens on 5433").performClick()
        rule.onNodeWithText("Edit").performClick()
        rule.onNodeWithContentDescription("Memory text").assertExists()
    }

    @Test
    fun aFailedLoadShowsAnErrorAndCanBeRetried() {
        var attempts = 0
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine {
            attempts++
            if (attempts == 1) {
                respond("{\"error\":\"unavailable\",\"message\":\"Memories are temporarily unavailable.\"}", HttpStatusCode.ServiceUnavailable, json)
            } else {
                respond("[${memory("m1", "Recovered memory")}]", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        rule.setContent { MyceliumTheme(true) { MemoriesSettings(store) } }
        rule.waitUntil(5_000) { shows("Memories are temporarily unavailable.") }
        rule.onNodeWithText("Retry").performClick()
        rule.waitUntil(5_000) { shows("Recovered memory") }
    }

    @Test
    fun aMemoryStoredByAnAgentAppearsWhileTheTabIsOpenAndFollowsItsEditAndRemoval() {
        val store = showMemories(listOf(memory("m1", "Staging listens on 5433")))
        rule.waitUntil(5_000) { shows("Staging listens on 5433") }

        stream(store, """{"type":"memory",${memory("m2", "Releases need a tag").removePrefix("{")}""")
        rule.waitUntil(5_000) { shows("Releases need a tag") }

        stream(store, """{"type":"memory",${memory("m2", "Releases need a signed tag").removePrefix("{")}""")
        rule.waitUntil(5_000) { shows("Releases need a signed tag") }
        rule.onNodeWithText("Releases need a tag").assertDoesNotExist()

        stream(store, """{"type":"memory_removed","memory_id":"m1"}""")
        rule.waitUntil(5_000) { !shows("Staging listens on 5433") }
    }

    @Test
    fun aMemoryThatArrivesDuringASearchIsShownOnlyWhenItMatches() {
        val stored = mutableListOf(memory("m1", "Staging listens on 5433"))
        val store = showMemories(stored)
        rule.onNodeWithContentDescription("Search memories").performTextInput("Releases")
        rule.waitUntil(5_000) { !shows("Staging listens on 5433") }

        stored += memory("m2", "Releases need a tag")
        stream(store, """{"type":"memory",${memory("m2", "Releases need a tag").removePrefix("{")}""")

        rule.waitUntil(5_000) { shows("Releases need a tag") }
        rule.onNodeWithText("Staging listens on 5433").assertDoesNotExist()
    }
}
