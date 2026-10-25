package mycelium.desktop

import androidx.compose.foundation.layout.height
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.buildThread
import mycelium.state.hasOlderEvents
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private fun questions(seqs: IntRange) = seqs.joinToString(",", "[", "]") { """{"conversation_id":"c1","seq":$it,"kind":"user_message","payload":{"text":"Question $it"}}""" }

class LazyHistoryTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requested = java.util.Collections.synchronizedList(mutableListOf<String>())

    private fun showLongConversation(): Store {
        val engine = MockEngine { request ->
            requested += request.url.toString()
            val body = when {
                !request.url.encodedPath.endsWith("/events") -> "[]"
                request.url.parameters["before"] == "21" -> questions(1..20)
                request.url.parameters["before"] != null -> questions(21..40)
                else -> "[]"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.open("c1") }

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()
                val scope = rememberCoroutineScope()

                ThreadView(
                    items = buildThread(state.events["c1"].orEmpty()),
                    onAnswer = { _, _ -> },
                    onStopWorkflow = {},
                    modifier = Modifier.height(200.dp),
                    hasOlder = state.hasOlderEvents("c1"),
                    loadingOlder = "c1" in state.loadingOlderEvents,
                    onLoadOlder = { scope.launch { store.loadOlder("c1") } },
                )
            }
        }

        return store
    }

    @Test
    fun aConversationLongerThanOnePageShowsTheNewestMessageWithoutLoadingTheFirst() {
        showLongConversation()

        rule.onNodeWithText("Question 40").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Question 1").fetchSemanticsNodes().size)
        assertTrue(requested.none { it.contains("before=21") })
    }

    @Test
    fun scrollingToTheTopLoadsTheEarlierMessages() {
        val store = showLongConversation()

        rule.onNodeWithContentDescription("Conversation").performScrollToIndex(19)
        rule.waitUntil(5_000) { store.state.value.events["c1"]?.size == 40 }
        rule.onNodeWithContentDescription("Conversation").performScrollToIndex(39)

        rule.onNodeWithText("Question 1").assertIsDisplayed()
    }
}
