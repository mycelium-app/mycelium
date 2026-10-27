package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.ChatToolInfo
import mycelium.api.ChatToolServer
import mycelium.api.IntegrationPolicy
import mycelium.state.ApprovalColumn
import mycelium.state.ApprovalPresets
import mycelium.state.ColumnAlign
import mycelium.state.GroupTick
import mycelium.state.askWord
import mycelium.state.changePreset
import mycelium.state.editWord
import mycelium.state.groupFollowsPreset
import mycelium.state.groupTick
import mycelium.state.presetOption
import mycelium.state.presetSummary
import mycelium.state.resetTool
import mycelium.state.resolveApproval
import mycelium.state.toggleGroup
import mycelium.state.toggleTool
import mycelium.state.toolIsCustomised

private sealed interface ApprovalRow {
    data class Group(val kind: String, val tools: List<ChatToolInfo>) : ApprovalRow

    data class Tool(val tool: ChatToolInfo) : ApprovalRow
}

private fun approvalRows(tools: List<ChatToolInfo>): List<ApprovalRow> = listOf("read", "write").flatMap { kind ->
    val group = tools.filter { (it.kind == "read") == (kind == "read") }

    if (group.isEmpty()) emptyList() else listOf<ApprovalRow>(ApprovalRow.Group(kind, group)) + group.map { ApprovalRow.Tool(it) }
}

private fun groupTitle(kind: String) = if (kind == "read") "Read" else "Write"

private val ToolColumn = Col("Tool", 120.dp, weight = 1f)
private val AskColumn = Col("Ask first", 96.dp, preferred = 112.dp)
private val EditColumn = Col("Edit response", 96.dp, preferred = 128.dp)

@Composable
internal fun ApprovalsSummary(account: ChatToolServer, onChange: () -> Unit) {
    val colors = MyTheme.colors
    val rows = approvalRows(account.tools)
    val policy = account.policy

    Section("Approvals") {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S4), verticalAlignment = Alignment.CenterVertically) {
            Label("Preset", style = Tokens.Type.Meta, color = colors.textMuted)
            Label(presetSummary(policy))
        }

        Row { MyButton(onChange, text = "Change approvals", variant = ButtonVariant.Outline) }

        Table(
            listOf(ToolColumn, AskColumn, EditColumn),
            rows.size,
            { row, column ->
                when (val entry = rows[row]) {
                    is ApprovalRow.Group -> if (column == 0) TableCell.Text(groupTitle(entry.kind), color = colors.textMuted) else TableCell.Text("")
                    is ApprovalRow.Tool -> when (column) {
                        0 -> TableCell.Text(entry.tool.name, Tokens.Type.Code)
                        1 -> TableCell.Text(askWord(policy, entry.tool), color = colors.textMuted)
                        else -> TableCell.Text(editWord(policy, entry.tool), color = colors.textMuted)
                    }
                }
            },
            lazy = false,
            rowKey = { index -> rows[index].let { if (it is ApprovalRow.Group) it.kind else (it as ApprovalRow.Tool).tool.name } },
        )
    }
}

@Composable
internal fun ApprovalsDialog(account: ChatToolServer, onDismiss: () -> Unit, onSave: (IntegrationPolicy) -> Unit) {
    val colors = MyTheme.colors
    var draft by remember { mutableStateOf(account.policy) }
    var customising by remember { mutableStateOf(false) }
    val rows = approvalRows(account.tools)

    Dialog(onDismiss) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Label("Approvals for ${accountTitle(account)}", Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)
            MyButton(onDismiss, icon = MyIcon.Close, description = "Close", size = ButtonSize.Sm, muted = true)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Preset", style = Tokens.Type.MetaStrong, color = colors.textMuted)
            PresetPicker(draft.preset) { draft = changePreset(draft, it) }
            Label(presetOption(draft.preset).hint, style = Tokens.Type.Meta, color = colors.textMuted)
        }

        MyButton(
            { customising = !customising },
            text = "Customise",
            icon = if (customising) MyIcon.ChevronDown else MyIcon.ChevronRight,
            size = ButtonSize.Sm,
            muted = true,
            description = if (customising) "Hide the customise table" else "Show the customise table",
            stateDescription = if (customising) "Expanded" else "Collapsed",
        )

        if (customising) {
            Label("A muted tick follows the preset or its group. A solid tick is set for that tool.", style = Tokens.Type.Meta, color = colors.textMuted)

            Table(
                listOf(ToolColumn, AskColumn, EditColumn, Col("", 64.dp, preferred = 64.dp, align = ColumnAlign.End)),
                rows.size,
                { row, column ->
                    when (val entry = rows[row]) {
                        is ApprovalRow.Group -> when (column) {
                            0 -> TableCell.Text("${groupTitle(entry.kind)} tools", Tokens.Type.UiStrong)
                            3 -> TableCell.Text("")
                            else -> {
                                val which = if (column == 1) ApprovalColumn.Ask else ApprovalColumn.Edit
                                val tick = groupTick(draft, which, entry.tools)

                                TableCell.Slot(measureAs = "Edit response") {
                                    Checkbox(
                                        checked = tick == GroupTick.All,
                                        partly = tick == GroupTick.Some,
                                        inherited = groupFollowsPreset(draft, which, entry.kind, entry.tools),
                                        description = "${if (which == ApprovalColumn.Ask) "Ask first" else "Edit response"} for all ${entry.kind} tools",
                                        onToggle = { draft = toggleGroup(draft, which, entry.kind, entry.tools) },
                                        checkedState = "On",
                                        uncheckedState = "Off",
                                    )
                                }
                            }
                        }

                        is ApprovalRow.Tool -> when (column) {
                            0 -> TableCell.Text(entry.tool.name, Tokens.Type.Code)
                            3 -> TableCell.Slot(measureAs = "Reset") {
                                if (toolIsCustomised(draft, entry.tool)) {
                                    MyButton({ draft = resetTool(draft, entry.tool) }, text = "Reset", size = ButtonSize.Sm, muted = true, description = "Reset ${entry.tool.name} to follow its group")
                                }
                            }
                            else -> {
                                val which = if (column == 1) ApprovalColumn.Ask else ApprovalColumn.Edit
                                val resolved = resolveApproval(draft, which, entry.tool)

                                TableCell.Slot(measureAs = "Edit response") {
                                    Checkbox(
                                        checked = resolved.value,
                                        inherited = !resolved.own,
                                        description = "${if (which == ApprovalColumn.Ask) "Ask first" else "Edit response"} for ${entry.tool.name}",
                                        onToggle = { draft = toggleTool(draft, which, entry.tool) },
                                        checkedState = "On",
                                        uncheckedState = "Off",
                                    )
                                }
                            }
                        }
                    }
                },
                lazy = false,
                rowKey = { index -> rows[index].let { if (it is ApprovalRow.Group) it.kind else (it as ApprovalRow.Tool).tool.name } },
            )
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton({ onSave(draft) }, text = "Save", variant = ButtonVariant.Primary)
        }
    }
}

@Composable
private fun PresetPicker(selected: String, onSelect: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }

    Box(Modifier.padding(0.dp)) {
        MyButton({ open = true }, text = presetOption(selected).name, trailingIcon = MyIcon.ChevronDown, variant = ButtonVariant.Outline, description = "Preset: ${presetOption(selected).name}", selected = open)

        if (open) {
            MenuPopup(onDismiss = { open = false }, width = 320.dp) {
                for (option in ApprovalPresets) {
                    MenuItem(option.name, { open = false; onSelect(option.id) }, checked = option.id == selected)
                }
            }
        }
    }
}
