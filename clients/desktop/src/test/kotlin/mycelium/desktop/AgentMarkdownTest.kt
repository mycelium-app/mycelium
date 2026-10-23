package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.state.ThreadItem
import mycelium.ui.LocalPlatform
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private class RecordingPlatform : Platform {
    val opened = mutableListOf<String>()
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) {
        opened += url
    }
    override fun copyToClipboard(text: String) = Unit
}

internal const val EverythingMessage = """# Release plan

### Third level

- [x] Build the **artefact**
- [ ] Sign it
  - nested step
  - [x] nested done
- plain bullet

3. third
4. fourth

Visit [the docs](https://example.com/docs) or https://example.org/raw. ***Both*** ~~old~~ and `code`.

![diagram](https://example.com/d.png)

> A quote with a list
>
> - inside

| Name | Count | Note |
|:-----|:-----:|-----:|
| alpha | 1 | left |
| beta | 22 | right |

```kotlin
// entry point
fun main() = println("hi ${'$'}{40 + 2}")
```

```diff
@@ -1 +1 @@
-old line
+new line
```

> [!WARNING]
> Back up first.

> [!TIP]
> Use `--dry-run`.

Water is H~2~O and E = mc^2^.

A source[^1].

[^1]: The original paper.

---

Done."""

class AgentMarkdownTest {
    @get:Rule
    val rule = createComposeRule()

    private val platform = RecordingPlatform()

    private fun show(text: String) {
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalPlatform provides platform) {
                    ThreadView(listOf(ThreadItem.AgentMessage("m1", text)), onAnswer = { _, _ -> }, onStopWorkflow = {})
                }
            }
        }
    }

    @Test
    fun taskListItemsReportDoneOrNotDoneAndShowNoBracketsOrBulletDot() {
        show("- [x] Build it\n- [ ] Sign it\n- [x] Ship it")

        rule.onAllNodesWithContentDescription("Done").assertCountEquals(2)
        rule.onAllNodesWithContentDescription("Not done").assertCountEquals(1)
        rule.onNodeWithText("Sign it").assertIsDisplayed()
        rule.onAllNodesWithText("[x]", substring = true).assertCountEquals(0)
        rule.onAllNodesWithText("[ ]", substring = true).assertCountEquals(0)
    }

    @Test
    fun nestedListItemsSitToTheRightOfTheirParent() {
        show("- parent\n  - child")

        val parent = rule.onNodeWithText("parent").getBoundsInRoot().left
        val child = rule.onNodeWithText("child").getBoundsInRoot().left

        assertTrue(child > parent)
    }

    @Test
    fun clickingALinkOpensItsUrlThroughThePlatform() {
        show("Read [the docs](https://example.com/docs) first")

        rule.onNodeWithText("Read the docs first").performClick()

        assertEquals(listOf("https://example.com/docs"), platform.opened)
    }

    @Test
    fun aCodeBlockShowsItsLanguageAndACopyButton() {
        show("```kotlin\nfun main() {}\n```")

        rule.onNodeWithText("kotlin").assertIsDisplayed()
        rule.onNodeWithContentDescription("Copy kotlin code").assertIsDisplayed()
    }

    @Test
    fun aCalloutNamesItsKindInTextAndKeepsItsContent() {
        show("> [!WARNING]\n> Back up first.")

        rule.onNodeWithText("Warning").assertIsDisplayed()
        rule.onNodeWithText("Back up first.").assertIsDisplayed()
        rule.onAllNodesWithText("[!WARNING]", substring = true).assertCountEquals(0)
    }

    @Test
    fun superscriptSubscriptAndAFootnoteReferenceCanShareAParagraph() {
        show("Water is H~2~O and E = mc^2^, with a source[^1].\n\n[^1]: The original paper.")

        rule.onNodeWithText("Water is H2O and E = mc2, with a source1.").assertIsDisplayed()
    }

    @Test
    fun footnotesListAtTheEndWithTheirLabelAndText() {
        show("Claim[^1].\n\n[^1]: The original paper.")

        rule.onNodeWithText("Claim1.").assertIsDisplayed()
        rule.onNodeWithText("The original paper.").assertIsDisplayed()
    }

    @Test
    fun aMessageWithEveryConstructRendersToAnImage() {
        for (dark in listOf(true, false)) {
            val file = renderAt("agent-markdown", dark, false) {
                MyTheme.colors.let { colors ->
                    Box(Modifier.fillMaxSize().background(colors.bgContent).padding(24.dp)) {
                        ThreadView(listOf(ThreadItem.AgentMessage("m1", EverythingMessage)), onAnswer = { _, _ -> }, onStopWorkflow = {})
                    }
                }
            }

            assertTrue(file.length() > 0)
        }
    }
}
