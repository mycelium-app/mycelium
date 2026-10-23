package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.onNodeWithContentDescription
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import mycelium.api.MyceliumApi
import mycelium.api.parseStreamMessage
import mycelium.state.Store
import mycelium.ui.MemoriesSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test

private fun memory(id: String, text: String) =
    """{"id":"$id","text":"$text","scopes":["global"],"created_by":"laptop/Fix login","conversation_id":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

class MemoriesLiveTest {
    @get:Rule
    val rule = createComposeRule()

    private fun showMemories(stored: List<String>): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val asked = request.url.parameters["query"].orEmpty()
            val matching = if (asked.isEmpty()) stored else stored.filter { asked in it }

            respond("[" + matching.joinToString(",") + "]", HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        rule.setContent { MyceliumTheme(true) { MemoriesSettings(store) } }

        return store
    }

    private fun shows(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun stream(store: Store, text: String) = store.receive(parseStreamMessage(text)!!)

    @Test
    fun aMemoryStoredByAnAgentAppearsWhileTheTabIsOpenAndFollowsItsEditAndRemoval() {
        val store = showMemories(listOf(memory("m1", "Staging listens on 5433")))
        rule.waitUntil { shows("Staging listens on 5433") }

        stream(store, """{"type":"memory",${memory("m2", "Releases need a tag").removePrefix("{")}""")
        rule.waitUntil { shows("Releases need a tag") }

        stream(store, """{"type":"memory",${memory("m2", "Releases need a signed tag").removePrefix("{")}""")
        rule.waitUntil { shows("Releases need a signed tag") }
        rule.onNodeWithText("Releases need a tag").assertDoesNotExist()

        stream(store, """{"type":"memory_removed","memory_id":"m1"}""")
        rule.waitUntil { !shows("Staging listens on 5433") }
    }

    @Test
    fun aMemoryThatArrivesDuringASearchIsShownOnlyWhenItMatches() {
        val stored = mutableListOf(memory("m1", "Staging listens on 5433"))
        val store = showMemories(stored)
        rule.onNodeWithContentDescription("Search memories").performTextInput("Releases")
        rule.waitUntil { !shows("Staging listens on 5433") }

        stored += memory("m2", "Releases need a tag")
        stream(store, """{"type":"memory",${memory("m2", "Releases need a tag").removePrefix("{")}""")

        rule.waitUntil { shows("Releases need a tag") }
        rule.onNodeWithText("Staging listens on 5433").assertDoesNotExist()
    }
}
