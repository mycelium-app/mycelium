package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.dp
import kotlinx.serialization.json.Json
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.state.buildThread
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test

private fun line(seq: Long, event: String, text: String, why: String) =
    Event("c1", seq, EventKinds.Environment, Json.parseToJsonElement("""{"event":"$event","text":"$text","why":"$why","at":"2026-10-28T14:02:00Z"}"""))

class EnvironmentThreadLinesTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun theThreadSaysWhenTheEnvironmentStartedWasTakenOverHandedBackAndStopped() {
        val events = listOf(
            line(1, "started", "Environment started on willow", "You can watch, or take over at any time."),
            line(2, "took_over", "You took over the environment", "The agent waits and does not click or type until you hand back."),
            line(3, "handed_back", "You handed back the environment", "The agent can see what changed."),
            line(4, "stopped", "Environment stopped", "You closed this conversation, so the environment and its apps closed too."),
        )

        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(640.dp).padding(16.dp)) {
                    ThreadView(buildThread(events), onAnswer = { _, _ -> }, onStopWorkflow = {}, agent = "claude")
                }
            }
        }

        rule.onNodeWithText("Environment started on willow", substring = true).assertExists()
        rule.onNodeWithText("You took over the environment", substring = true).assertExists()
        rule.onNodeWithText("You handed back the environment", substring = true).assertExists()
        rule.onNodeWithText("Environment stopped", substring = true).assertExists()
        rule.onNodeWithText("so the environment and its apps closed too", substring = true).assertExists()
    }
}
