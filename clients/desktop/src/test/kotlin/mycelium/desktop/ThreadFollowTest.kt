package mycelium.desktop

import androidx.compose.foundation.layout.height
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.dp
import mycelium.state.ThreadItem
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test

class ThreadFollowTest {
    @get:Rule
    val rule = createComposeRule()

    private fun messages(count: Int) = (1..count).map { ThreadItem.AgentMessage("m$it", "Message $it") }

    @Test
    fun aNewMessageScrollsIntoViewWhenTheNewestIsShowing() {
        val items = mutableStateOf<List<ThreadItem>>(messages(30))
        rule.setContent { MyceliumTheme(true) { ThreadView(items.value, onAnswer = { _, _ -> }, onStopWorkflow = {}, modifier = Modifier.height(300.dp)) } }

        items.value = messages(31)

        rule.onNodeWithText("Message 31").assertIsDisplayed()
    }
}
