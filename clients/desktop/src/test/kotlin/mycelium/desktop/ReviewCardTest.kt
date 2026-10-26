package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.serialization.json.Json
import mycelium.api.Event
import mycelium.state.buildThread
import mycelium.ui.LocalReviewActions
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.ReviewActions
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private val Items = Json.parseToJsonElement("""[{"sender":"Grace","place":{"channel":"general","topic":"lunch"},"content":"Noon?"},{"sender":"Linus","place":{"channel":"general","topic":"lunch"},"content":"Pizza?"}]""")
    .let { (it as kotlinx.serialization.json.JsonArray).toList() }

private fun requested() = Event("c1", 1, "review_request", Json.parseToJsonElement("""{"request_id":"r1","server":"Zulip","tool":"get_messages","count":2}"""))

class ReviewCardTest {
    @get:Rule
    val rule = createComposeRule()

    private val answers = CopyOnWriteArrayList<String>()

    private fun show(events: List<Event>) {
        val items = buildThread(events)
        rule.setContent {
            MyceliumTheme(true) {
                val actions = ReviewActions(load = { Items }, answer = { _, decision, removed -> answers += "$decision:${removed.joinToString(",")}" })

                CompositionLocalProvider(LocalReviewActions provides actions) {
                    Box(Modifier.width(640.dp).background(MyTheme.colors.bgContent).padding(16.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}) }
                }
            }
        }
    }

    @Test
    fun aHeldResultListsEachItemAsATickedRowAndSharingSendsEverything() {
        show(listOf(requested()))

        rule.onNodeWithText("Waiting for your review").assertExists()
        rule.onNodeWithText("Grace, general / lunch: Noon?").assertExists()
        rule.onNodeWithContentDescription("Share item 2 with the agent").assertExists()
        rule.onNodeWithContentDescription("Share the whole result with the agent").performClick()
        rule.waitForIdle()

        assertEquals(listOf("share:"), answers.toList())
    }

    @Test
    fun untickingAnItemAndRemovingSendsItsPosition() {
        show(listOf(requested()))

        rule.onNodeWithContentDescription("Share item 2 with the agent").performClick()
        rule.onNodeWithContentDescription("Share the result without the unticked items").performClick()
        rule.waitForIdle()

        assertEquals(listOf("remove:1"), answers.toList())
    }

    @Test
    fun removingWithEverythingTickedSaysWhatToDoAndSendsNothing() {
        show(listOf(requested()))

        rule.onNodeWithContentDescription("Share the result without the unticked items").performClick()
        rule.waitForIdle()

        rule.onNodeWithText("Untick the items to remove.").assertExists()
        assertEquals(emptyList(), answers.toList())
    }

    @Test
    fun withholdingSendsTheDecisionAndAnAnsweredCardSaysWhatHappened() {
        show(listOf(requested()))

        rule.onNodeWithContentDescription("Withhold this result from the agent").performClick()
        rule.waitForIdle()
        assertEquals(listOf("withhold:"), answers.toList())

        show(listOf(requested(), Event("c1", 2, "review_resolved", Json.parseToJsonElement("""{"request_id":"r1","decision":"withhold","removed_count":0}"""))))
        rule.onNodeWithText("Withheld from the agent").assertExists()
    }

    @Test
    fun theCardRenders() {
        val items = buildThread(listOf(requested()))
        val actions = ReviewActions(load = { Items }, answer = { _, _, _ -> })

        for (dark in listOf(true, false)) {
            val image = renderAt("review-card", dark, false) {
                CompositionLocalProvider(LocalReviewActions provides actions) {
                    Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent).padding(16.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}) }
                }
            }

            assertEquals(true, image.length() > 5_000)
        }
    }
}
