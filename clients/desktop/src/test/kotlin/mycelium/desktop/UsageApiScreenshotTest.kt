package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlin.test.Test
import kotlin.test.assertTrue
import kotlin.time.Clock
import kotlin.time.Duration.Companion.days
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds
import mycelium.api.ApiCost
import mycelium.api.ModelProviderUsage
import mycelium.api.MyceliumApi
import mycelium.api.ResetCredit
import mycelium.api.ResetCredits
import mycelium.api.UsageWindow
import mycelium.state.AgentAccount
import mycelium.state.Store
import mycelium.ui.ModelProvidersSettings
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.UsageCard
import org.junit.Rule

private fun androidx.compose.ui.test.SemanticsNodeInteraction.save(name: String) {
    val png = org.jetbrains.skia.Image.makeFromBitmap(captureToImage().asSkiaBitmap()).encodeToData(org.jetbrains.skia.EncodedImageFormat.PNG)!!.bytes
    val out = java.io.File("build/screens/$name.png")

    out.parentFile.mkdirs()
    out.writeBytes(png)
}

class UsageApiScreenshotTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    @Test
    fun theCompactCardFitsThreeAccountsAndAnApiRowInAboutTwoHundredPixels() {
        val now = Clock.System.now()
        val read = (now - 2.minutes).toString()
        val windows = { five: Int, fiveReset: Long, week: Int, weekDays: Long, runsOut: Boolean ->
            listOf(
                UsageWindow("five_hour", five.toDouble(), resetsAt = (now + fiveReset.minutes + 30.seconds).toString()),
                UsageWindow("seven_day", week.toDouble(), resetsAt = (now + weekDays.days).toString(), ratePerHour = 1.0, runsOutAt = if (runsOut) (now + 2.days).toString() else null),
            )
        }
        val accounts = listOf(
            AgentAccount("claude", "ann@example.com", "oauth_token", emptyList(), windows(38, 160, 71, 4, true), read) to true,
            AgentAccount("claude", "ann@work.example.com", "oauth_token", emptyList(), windows(64, 70, 22, 5, false), read) to false,
            AgentAccount("codex", "sam@sam.example.com", "device_auth", emptyList(), windows(12, 245, 96, 6, false), read, ResetCredits(2, listOf(ResetCredit("a", "2099-11-14T09:00:00Z")))) to false,
        )

        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.background(MyTheme.colors.bgPanel).padding(6.dp)) {
                    Box(Modifier.width(268.dp)) {
                        UsageCard(accounts, providers = listOf(ModelProviderUsage("p1", "Anthropic", read, apiCost = ApiCost(6.4, 142.18, 1_200_000))), onOpenHistory = {}, onUseReset = { _, _ -> "reset" })
                    }
                }
            }
        }

        rule.onNodeWithText("This chat").getUnclippedBoundsInRoot()
        rule.onRoot().save("usage-card-compact")
        assertTrue(rule.onRoot().getUnclippedBoundsInRoot().let { it.bottom - it.top } <= 220.dp, "the card should be compact")
    }

    @Test
    fun theAdminKeyDialogShowsItsErrorSummary() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val provider = """{"id":"p2","kind":"anthropic","name":"Anthropic","key_hint":"f00d","has_admin_key":true,"admin_key_hint":"9c1e","models":["claude-sonnet-5-5"],"created_at":"2026-10-22T10:00:00Z"}"""
        val engine = MockEngine { respond("[$provider]", HttpStatusCode.OK, json) }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Anthropic").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for Anthropic").performClick()
        rule.onNodeWithText("Change admin key").performClick()
        rule.onNodeWithContentDescription("Admin API key (optional)").performTextInput("sk-ant-api03-example-not-admin")
        rule.onNodeWithText("Save").performClick()

        rule.onNodeWithText("There is a problem").getUnclippedBoundsInRoot()
        rule.onAllNodes(isRoot()).onLast().save("admin-key-dialog-error")
    }
}
