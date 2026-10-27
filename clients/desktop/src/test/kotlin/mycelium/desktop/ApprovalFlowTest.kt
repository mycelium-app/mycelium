package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.ResponseFilterResult
import mycelium.api.ResponseToEdit
import mycelium.state.PermissionOption
import mycelium.state.ThreadItem
import mycelium.ui.LocalResponseActions
import mycelium.ui.MyceliumTheme
import mycelium.ui.ResponseActions
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private val Options = listOf(
    PermissionOption("approve", "Approve", "allow_once"),
    PermissionOption("approve_edit", "Approve and edit response", "allow_once"),
    PermissionOption("always", "Always approve", "allow_always"),
    PermissionOption("deny", "Deny", "reject_once"),
)

private fun message(id: Int) = JsonObject(mapOf("id" to JsonPrimitive(id), "text" to JsonPrimitive("Message $id")))

class ApprovalFlowTest {
    @get:Rule
    val rule = createComposeRule()

    private val answers = mutableListOf<String>()
    private val sent = mutableListOf<JsonElement>()

    private fun showPermission(editsByDefault: Boolean) {
        val item = ThreadItem.Permission("k", "r1", "mcp__zulip__get_messages", Options, false, "Zulip", "ada@example.com", null, editsByDefault)
        rule.setContent { MyceliumTheme(true) { Box(Modifier.width(640.dp)) { ThreadView(listOf(item), onAnswer = { _, option -> answers += option }, onStopWorkflow = {}) } } }
    }

    private fun showResponse(rows: Int) {
        val actions = ResponseActions(
            load = { ResponseToEdit("get_messages", "Zulip", "ada@example.com", null, null, JsonArray((1..rows).map(::message))) },
            filter = { _, _ -> ResponseFilterResult() },
            send = { _, value -> sent += value },
        )
        val item = ThreadItem.ResponseEdit("k", "r1", "Zulip", "ada@example.com", "get_messages")
        rule.setContent { MyceliumTheme(true) { CompositionLocalProvider(LocalResponseActions provides actions) { Box(Modifier.width(900.dp)) { ThreadView(listOf(item), onAnswer = { _, _ -> }, onStopWorkflow = {}) } } } }
    }

    @Test
    fun approveIsPrimaryAndTheChevronOffersEditAndAlways() {
        showPermission(editsByDefault = false)

        rule.onNodeWithContentDescription("More ways to approve").performClick()
        rule.onNodeWithText("Approve and edit response").assertExists()
        rule.onNodeWithText("Stop asking for get_messages").assertExists()
        rule.onNodeWithText("Always approve").performClick()

        assertEquals(listOf("always"), answers)
    }

    @Test
    fun theEditSettingMakesApproveAndEditResponseThePrimaryAction() {
        showPermission(editsByDefault = true)

        rule.onNodeWithContentDescription("More ways to approve and edit response").performClick()
        rule.onNodeWithText("Agent sees the response as it is").assertExists()
        rule.onNodeWithText("Approve").performClick()

        assertEquals(listOf("approve"), answers)
    }

    @Test
    fun denyIsASeparateButton() {
        showPermission(editsByDefault = false)

        rule.onNodeWithContentDescription("Deny mcp__zulip__get_messages").performClick()

        assertEquals(listOf("deny"), answers)
    }

    @Test
    fun clickingARowDropsItAndSendToAgentSendsOnlyTheKeptRows() {
        showResponse(3)
        rule.waitUntil(5_000) { rule.onAllNodesWithDescription("Keep row 1") }
        rule.onNodeWithContentDescription("Keep row 2").performClick()
        rule.onNodeWithText("2 of 3 kept", substring = true).assertExists()
        rule.onNodeWithText("Send to agent").performClick()

        rule.waitUntil(5_000) { sent.isNotEmpty() }
        assertEquals(JsonArray(listOf(message(1), message(3))), sent.single())
    }

    @Test
    fun selectNoneThenSendNothingSendsNull() {
        showResponse(3)
        rule.waitUntil(5_000) { rule.onAllNodesWithDescription("Keep row 1") }
        rule.onNodeWithText("Select none").performClick()
        rule.onNodeWithText("0 of 3 kept", substring = true).assertExists()
        rule.onNodeWithText("Send nothing").performClick()

        rule.waitUntil(5_000) { sent.isNotEmpty() }
        assertEquals(JsonNull, sent.single())
    }
}

private fun androidx.compose.ui.test.junit4.ComposeContentTestRule.onAllNodesWithDescription(description: String): Boolean =
    onAllNodes(androidx.compose.ui.test.hasContentDescription(description)).fetchSemanticsNodes().isNotEmpty()
