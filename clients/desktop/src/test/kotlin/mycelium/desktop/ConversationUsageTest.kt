package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test

private const val UsedConversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Work","status":"working","created_at":"2026-10-24T10:00:00Z","updated_at":"2026-10-24T10:00:00Z"}"""
private const val ConversationSpend = """{"context_used":52000,"context_size":200000,"input_tokens":110,"output_tokens":55,"cache_read_tokens":1000,"cache_write_tokens":0,"cost_usd":0.25,"cost_estimated":false}"""

class ConversationUsageTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    @Test
    fun aNarrowConversationShowsHowFullItsContextIsAndWhatItHasSpentSeparately() {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> "[$UsedConversation]"
                path.endsWith("/conversations/c1") -> UsedConversation
                path.endsWith("/conversations/c1/usage") -> ConversationSpend
                path.endsWith("/events") || path.endsWith("/nodes") || path.endsWith("/artifacts") || path.endsWith("/queue") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh(); store.open("c1") }
        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()
                Box(Modifier.width(320.dp)) {
                    ConversationScreen(store, state, "c1", onArchived = {}, panels = false)
                }
            }
        }

        rule.waitUntil { rule.onAllNodesWithText("Spent", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Context 52k of 200k (26%)").assertIsDisplayed()
        rule.onNodeWithText("Spent ~$0.25").assertIsDisplayed()
    }
}
