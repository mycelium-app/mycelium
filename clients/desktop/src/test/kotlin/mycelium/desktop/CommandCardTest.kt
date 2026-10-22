package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import kotlinx.serialization.json.Json
import mycelium.api.Event
import mycelium.state.buildThread
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test

class CommandCardTest {
    @get:Rule
    val rule = createComposeRule()

    private fun update(seq: Long, body: String) = Event("c1", seq, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":$body}"""))

    @Test
    fun aCommandShowsWhatRanAndWhatItPrinted() {
        val items = buildThread(
            listOf(
                update(1, """{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Print one","kind":"execute","status":"completed","rawInput":{"command":"echo 1"}}"""),
                update(2, """{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"1\n"}}]}"""),
            ),
        )
        rule.setContent { MyceliumTheme(true) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}) } }

        rule.onNodeWithContentDescription("Command echo 1").assertExists()
        rule.onNodeWithText("1", useUnmergedTree = true).assertExists()
    }
}
