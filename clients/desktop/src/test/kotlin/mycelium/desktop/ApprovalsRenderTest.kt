package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.ResponseFilterResult
import mycelium.api.ResponseToEdit
import mycelium.state.PermissionOption
import mycelium.state.ThreadItem
import mycelium.ui.LocalResponseActions
import mycelium.ui.ResponseActions
import mycelium.ui.ThreadView
import kotlin.test.Test
import kotlin.test.assertTrue

class ApprovalsRenderTest {
    private val options = listOf(
        PermissionOption("approve", "Approve", "allow_once"),
        PermissionOption("approve_edit", "Approve and edit response", "allow_once"),
        PermissionOption("always", "Always approve", "allow_always"),
        PermissionOption("deny", "Deny", "reject_once"),
    )

    private fun actions(value: JsonElement) = ResponseActions(
        load = { ResponseToEdit("get_messages", "Zulip", "ada@example.com", null, null, value) },
        filter = { _, _ -> ResponseFilterResult() },
        send = { _, _ -> },
    )

    private fun row(id: Int) = JsonObject(mapOf("id" to JsonPrimitive(id), "sender" to JsonPrimitive("Ada"), "text" to JsonPrimitive("Message $id")))

    private fun render(name: String, vararg items: ThreadItem, value: JsonElement = JsonArray((1..6).map(::row))) =
        renderAt(name, dark = true, phone = false) {
            CompositionLocalProvider(LocalResponseActions provides actions(value)) {
                Box(Modifier.width(900.dp)) { ThreadView(items.toList(), onAnswer = { _, _ -> }, onStopWorkflow = {}) }
            }
        }

    @Test
    fun theApprovalCardRenders() {
        val item = ThreadItem.Permission("k", "r1", "mcp__zulip__get_messages", options, false, "Zulip", "ada@example.com", null, false)

        assertTrue(render("approval-card", item).length() > 0)
    }

    @Test
    fun theResponseEditorRendersAListAsATable() {
        val item = ThreadItem.ResponseEdit("k", "r1", "Zulip", "ada@example.com", "get_messages")

        assertTrue(render("response-editor-list", item).length() > 0)
    }

    @Test
    fun theResponseEditorRendersAnObjectAsATree() {
        val item = ThreadItem.ResponseEdit("k", "r1", "Zulip", "ada@example.com", "get_messages")
        val value = JsonObject(mapOf("id" to JsonPrimitive(1), "sender" to JsonObject(mapOf("name" to JsonPrimitive("Ada"))), "text" to JsonPrimitive("Hello")))

        assertTrue(render("response-editor-tree", item, value = value).length() > 0)
    }
}
