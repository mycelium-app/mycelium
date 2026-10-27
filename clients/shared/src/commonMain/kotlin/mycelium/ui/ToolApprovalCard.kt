package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import mycelium.state.PermissionOption
import mycelium.state.ThreadItem
import mycelium.state.chatToolName

private const val Approve = "approve"
private const val ApproveEdit = "approve_edit"
private const val Always = "always"

data class ApprovalChoices(val primary: PermissionOption?, val alternatives: List<Pair<PermissionOption, String?>>, val deny: PermissionOption?)

private fun isAllow(option: PermissionOption) = option.kind.startsWith("allow")

private fun isAlways(option: PermissionOption) = option.id == Always || option.kind == "allow_always"

private fun isDeny(option: PermissionOption) = option.kind.startsWith("reject")

fun approvalChoices(permission: ThreadItem.Permission): ApprovalChoices {
    val allowing = permission.options.filter(::isAllow)
    val always = allowing.firstOrNull(::isAlways)
    val edit = allowing.firstOrNull { it.id == ApproveEdit }
    val approve = allowing.firstOrNull { it.id == Approve } ?: allowing.firstOrNull { !isAlways(it) && it.id != ApproveEdit }
    val primary = if (edit != null && permission.editsByDefault) edit else approve ?: edit ?: always
    val tool = chatToolName(permission.title)?.tool ?: permission.title
    val others = allowing.filter { it != primary && it != approve && it != edit && it != always }
    val alternatives = buildList {
        if (primary == edit && approve != null) {
            add(approve to "Agent sees the response as it is")
        }

        if (primary == approve && edit != null) {
            add(edit to "Hold the response so you can edit it first")
        }

        if (always != null && primary != always) {
            add(always to "Stop asking for $tool")
        }

        others.forEach { add(it to null) }
    }

    return ApprovalChoices(primary, alternatives, permission.options.firstOrNull(::isDeny))
}

private fun label(option: PermissionOption): String = when {
    option.id == Approve -> "Approve"
    option.id == ApproveEdit -> "Approve and edit response"
    isAlways(option) -> "Always approve"
    isDeny(option) -> "Deny"
    isAllow(option) -> "Approve"
    else -> option.name
}

@Composable
internal fun ToolApprovalCard(permission: ThreadItem.Permission, onAnswer: (String, String) -> Unit) {
    val colors = MyTheme.colors
    val choices = approvalChoices(permission)
    val arguments = permission.filter?.let { JsonObject(permission.input.orEmpty() + ("filter" to JsonPrimitive(it))) } ?: permission.input
    val server = permission.server?.let { listOfNotNull(it, permission.account).joinToString(" · ") }

    Card {
        CardHead {
            StatusMark(if (permission.resolved) "Answered" else if (permission.server != null) "Waiting for your approval" else "Needs your answer", if (permission.resolved) Tone.Muted else Tone.Attention, if (permission.resolved) MyIcon.Check else MyIcon.Alert)
        }

        Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            Label(permission.title, style = Tokens.Type.Code)

            if (server != null) {
                Label("Server: $server", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            ArgumentLines(arguments)
        }

        if (!permission.resolved) {
            Hairline()

            Row(
                Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val primary = choices.primary

                if (primary != null) {
                    SplitButton(
                        text = label(primary),
                        onClick = { onAnswer(permission.requestId, primary.id) },
                        alternatives = choices.alternatives.map { (option, hint) -> SplitAlternative(label(option), { onAnswer(permission.requestId, option.id) }, hint) },
                        description = "${label(primary)} ${permission.title}",
                        menuWidth = 340.dp,
                    )
                }

                choices.deny?.let { deny ->
                    MyButton({ onAnswer(permission.requestId, deny.id) }, text = label(deny), variant = ButtonVariant.Outline, description = "Deny ${permission.title}")
                }
            }
        }
    }
}
