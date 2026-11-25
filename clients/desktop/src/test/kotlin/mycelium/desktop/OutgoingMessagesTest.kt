package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import mycelium.state.Delivery
import mycelium.state.ThreadItem
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.OutboxControls
import mycelium.ui.ThreadView
import org.junit.Rule
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals

class OutgoingMessagesTest {
    @get:Rule
    val rule = createComposeRule()

    private val platform = AppsPlatform()
    private val retried = mutableListOf<String>()
    private val discarded = mutableListOf<String>()

    private fun show() {
        val items = listOf(
            ThreadItem.UserMessage("outgoing:a", "And which one should the nightly job use?", delivery = Delivery("a", null)),
            ThreadItem.UserMessage("outgoing:b", "Keep 14 days of them.", delivery = Delivery("b", "You are offline.")),
        )

        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalPlatform provides platform) {
                    Box(Modifier.size(720.dp, 400.dp)) {
                        ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}, runsOnModel = false, outbox = OutboxControls(retry = { retried += it }, discard = { discarded += it }))
                    }
                }
            }
        }
    }

    @Test
    fun aMessageOnItsWaySaysSendingAndAnUnsentOneSaysWhyWithRetryAndDelete() {
        show()

        rule.onNodeWithText("Sending").assertIsDisplayed()
        rule.onNodeWithText("Not sent. You are offline.").assertIsDisplayed()
        val png = org.jetbrains.skia.Image.makeFromBitmap(rule.onRoot().captureToImage().asSkiaBitmap()).encodeToData(org.jetbrains.skia.EncodedImageFormat.PNG)!!.bytes
        File("build/screens/2.63-messages-sending-and-not-sent.png").apply { parentFile.mkdirs() }.writeBytes(png)
    }

    @Test
    fun retryAndDeleteActOnTheUnsentMessage() {
        show()

        rule.onNodeWithText("Retry").performClick()
        rule.onNodeWithText("Delete").performClick()

        assertEquals(listOf("b") to listOf("b"), retried to discarded)
    }
}
