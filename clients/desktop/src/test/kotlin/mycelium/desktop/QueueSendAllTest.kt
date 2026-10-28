package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.MyceliumApi
import mycelium.api.QueuedPrompt
import mycelium.state.SessionOptions
import mycelium.ui.Composer
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class QueueSendAllTest {
    @get:Rule
    val rule = createComposeRule()

    private val requests = CopyOnWriteArrayList<Pair<HttpMethod, String>>()

    private val api = MyceliumApi(
        "https://mycelium.test",
        null,
        MockEngine { request ->
            requests += request.method to request.url.encodedPath
            respond("", HttpStatusCode.NoContent)
        },
    )

    @Test
    fun sendAllNowPostsToTheSendAllRoute() {
        val conversation = Conversation("c1", "u", "n", "claude", "/work", "t", ConversationStatus.Working, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")
        val queue = listOf(
            QueuedPrompt("q1", "c1", 1, "Open a merge request", "2026-10-22T10:00:00Z"),
            QueuedPrompt("q2", "c1", 2, "Check the Android strings", "2026-10-22T10:00:00Z"),
        )

        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = true, queue = queue, sending = emptyList(), onSend = {}, onStop = {},
                    onSendQueuedNow = {}, onEditQueued = {}, onRemoveQueued = {},
                    options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                    onSendAllQueuedNow = { runBlocking { api.sendAllQueuedNow("c1") } },
                )
            }
        }

        rule.onNodeWithText("Send all now").performClick()

        assertEquals(listOf(HttpMethod.Post to "/api/v1/conversations/c1/queue/send-all-now"), requests)
    }
}
