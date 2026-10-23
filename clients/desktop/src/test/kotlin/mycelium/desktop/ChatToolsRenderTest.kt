package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import mycelium.state.buildThread
import mycelium.ui.MyTheme
import mycelium.ui.ThreadView
import kotlin.test.Test
import kotlin.test.assertTrue

class ChatToolsRenderTest {
    private fun render(name: String, dark: Boolean) = renderAt(name, dark, false) {
        Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) {
            ThreadView(buildThread(ChatToolThread), onAnswer = { _, _ -> }, onStopWorkflow = {}, agent = "chat")
        }
    }

    @Test
    fun aChatWithAPendingApprovalAndFinishedToolCallsRenders() {
        assertTrue(render("chat-tools", true).length() > 5_000)
        assertTrue(render("chat-tools", false).length() > 5_000)
    }
}
