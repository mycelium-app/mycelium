package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import mycelium.state.ThreadItem
import mycelium.ui.GeneratedUiActions
import mycelium.ui.LocalGeneratedUiActions
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import mycelium.ui.Tokens
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private fun reply(block: String) = "Here are the options.\n\n```mycelium-ui\n$block\n```"

private const val Interface = """{"type":"column","children":[
  {"type":"heading","text":"Restore time for 2 TB"},
  {"type":"text","text":"Differential is the middle option."},
  {"type":"table","columns":["Plan","Needs"],"rows":[["Full","One backup"],["Incremental","Every backup"]]},
  {"type":"bar_chart","title":"Hours","bars":[{"label":"Full","value":4.1,"display":"4.1 h"},{"label":"Incremental","value":9.2,"display":"9.2 h"}]},
  {"type":"row","children":[
    {"type":"button","label":"Use differential","action":{"type":"send_message","text":"Use differential"}},
    {"type":"button","label":"Explain the trade-off","action":{"type":"side_chat","text":"Explain the trade-off"}},
    {"type":"button","label":"Open the plan","action":{"type":"open_file","path":"docs/restore.md"}}
  ]}
]}"""

class GeneratedUiTest {
    @get:Rule
    val rule = createComposeRule()

    private val sent = mutableListOf<String>()
    private val sideChats = mutableListOf<String>()

    private fun thread(text: String, canOpenFiles: Boolean = true) {
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalGeneratedUiActions provides GeneratedUiActions({ sent += it }, { sideChats += it }, canOpenFiles)) {
                    ThreadView(listOf(ThreadItem.AgentMessage("a", text)), onAnswer = { _, _ -> }, onStopWorkflow = {})
                }
            }
        }
    }

    @Test
    fun theBlockRendersAsAnInterfaceInsteadOfCode() {
        thread(reply(Interface))

        rule.onNodeWithText("Restore time for 2 TB").assertIsDisplayed()
        rule.onNodeWithText("Differential is the middle option.").assertIsDisplayed()
        rule.onNodeWithText("One backup").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("\"type\"", substring = true).fetchSemanticsNodes().size)
    }

    @Test
    fun theBarChartHasATextEquivalentAndShowsItsValues() {
        thread(reply(Interface))

        rule.onNodeWithContentDescription("Hours. Full: 4.1 h. Incremental: 9.2 h").assertExists()
        rule.onNodeWithText("9.2 h").assertIsDisplayed()
    }

    @Test
    fun anEmptyProgressBarStillShowsItsTrack() {
        thread(reply("""{"type":"progress","label":"Empty","value":0,"max":10,"display":"0 of 10"}"""))

        val pixels = rule.onNodeWithContentDescription("Empty: 0 of 10").captureToImage().toPixelMap()

        assertEquals(Tokens.Dark.borderControl, pixels[pixels.width / 2, pixels.height - 2])
    }

    @Test
    fun aProgressBarIsAnnouncedWithItsLabelValueAndRange() {
        thread(reply("""{"type":"progress","label":"Migrated tables","value":3,"max":8,"display":"3 of 8"}"""))

        rule.onNodeWithContentDescription("Migrated tables: 3 of 8").assert(SemanticsMatcher.expectValue(SemanticsProperties.ProgressBarRangeInfo, ProgressBarRangeInfo(0.375f, 0f..1f)))
        rule.onNodeWithText("3 of 8", useUnmergedTree = true).assertIsDisplayed()
    }

    @Test
    fun aSendMessageButtonSendsItsText() {
        thread(reply(Interface))

        rule.onNodeWithText("Use differential").performClick()

        assertEquals(listOf("Use differential"), sent)
    }

    @Test
    fun aSideChatButtonStartsASideChatWithItsText() {
        thread(reply(Interface))

        rule.onNodeWithText("Explain the trade-off").performClick()

        assertEquals(listOf("Explain the trade-off"), sideChats)
    }

    @Test
    fun anOpenFileButtonInAWorkspaceShowsThePath() {
        thread(reply(Interface))

        assertEquals(0, rule.onAllNodesWithText("docs/restore.md").fetchSemanticsNodes().size)

        rule.onNodeWithText("Open the plan").performClick()

        rule.onNodeWithText("docs/restore.md").assertIsDisplayed()
    }

    @Test
    fun anOpenFileButtonInAChatIsOnlyThePath() {
        thread(reply(Interface), canOpenFiles = false)

        rule.onNodeWithText("Open the plan: docs/restore.md").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Open the plan").fetchSemanticsNodes().size)
    }

    @Test
    fun invalidJsonFallsBackToTheCodeBlockWithANote() {
        thread(reply("{\"type\":\"column\""))

        rule.onNodeWithText("{\"type\":\"column\"").assertIsDisplayed()
        rule.onNodeWithText("This block could not be shown as an interface.").assertIsDisplayed()
    }

    @Test
    fun anOrdinaryCodeBlockHasNoNote() {
        thread("```kotlin\nval x = 1\n```")

        rule.onNodeWithText("val x = 1").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("could not be shown", substring = true).fetchSemanticsNodes().size)
    }
}
