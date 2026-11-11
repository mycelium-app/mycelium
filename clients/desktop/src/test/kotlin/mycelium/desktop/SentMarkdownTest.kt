package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import mycelium.api.Attachment
import mycelium.api.Event
import mycelium.state.ThreadItem
import mycelium.state.buildThread
import mycelium.state.pastedBlock
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class SentMarkdownTest {
    @get:Rule
    val rule = createComposeRule()

    private val platform = AppsPlatform()

    private fun show(items: List<ThreadItem>) {
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalPlatform provides platform) {
                    ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}, runsOnModel = false)
                }
            }
        }
    }

    @Test
    fun persistedMessagesRenderFormattingListsAndCode() {
        val text = "**Review** the `plan`\n\n- [x] First step\n\n```text\nsample command\n```"
        val event = Event("c1", 1, "user_message", buildJsonObject { put("text", text) })
        show(buildThread(listOf(event)))

        rule.onNodeWithText("Review the plan").assertIsDisplayed()
        val formatted = rule.onNodeWithText("Review the plan").fetchSemanticsNode().config[androidx.compose.ui.semantics.SemanticsProperties.Text].single()
        kotlin.test.assertTrue(formatted.spanStyles.any { (it.item.fontWeight?.weight ?: 400) > 400 })
        rule.onNodeWithText("First step").assertIsDisplayed()
        rule.onNodeWithContentDescription("Done").assertIsDisplayed()
        rule.onNodeWithContentDescription("Copy text code").assertIsDisplayed()
        rule.onNodeWithText(text).assertDoesNotExist()
    }

    @Test
    fun optimisticMessagesRenderLinksAndKeepPastedContentAndAttachments() {
        val pasted = "**literal pasted text**"
        val text = "- [x] Check notes\n\n[the docs](https://example.com/docs)\n${pastedBlock(pasted)}"
        show(listOf(ThreadItem.UserMessage("sending", text, files = listOf(Attachment("abc", "notes.txt", "text/plain", 12)))))

        rule.onNodeWithContentDescription("Done").assertIsDisplayed()
        rule.onNodeWithText("the docs").performClick()
        assertEquals(listOf("https://example.com/docs"), platform.opened)
        rule.onNodeWithText("notes.txt").assertIsDisplayed()
        rule.onNodeWithText("Pasted text · 1 line", substring = true).performClick()
        rule.onNodeWithText(pasted).assertIsDisplayed()
    }

    @Test
    fun rawHtmlRemainsLiteralText() {
        show(listOf(ThreadItem.UserMessage("sending", "<b>plain text</b>")))

        rule.onNodeWithText("<b>plain text</b>").assertIsDisplayed()
    }
}
