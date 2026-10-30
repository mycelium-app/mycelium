package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.Role
import mycelium.state.PlanEntry
import mycelium.state.PlanStatus
import mycelium.state.todoCurrentIndex

@Composable
fun TodoList(entries: List<PlanEntry>, conversationId: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    var expanded by remember(conversationId) { mutableStateOf(false) }
    val currentIndex = todoCurrentIndex(entries)
    val current = entries[currentIndex]
    val count = "${currentIndex + 1} of ${entries.size}"

    Column(modifier.fillMaxWidth().semantics { contentDescription = "To-do list" }) {
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = rowHeight())
                .pressable({ expanded = !expanded }, role = Role.Button, description = "To-do list, $count", shape = ControlShape, stateDescription = if (expanded) "Expanded" else "Collapsed")
                .padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(if (expanded) MyIcon.TodoNow else todoIcon(current.status), if (!expanded && current.status == PlanStatus.Running) colors.statusWorking else colors.textMuted, Tokens.Size.IconSm)
            Label(if (expanded) "To-do" else current.content, Modifier.weight(1f), color = if (expanded) colors.textMuted else colors.text, maxLines = 1)
            Label(count, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            Icon(if (expanded) MyIcon.ChevronDown else MyIcon.ChevronUp, colors.textMuted, Tokens.Size.IconSm)
        }

        if (expanded) {
            Column(Modifier.fillMaxWidth().padding(start = Tokens.Space.S3, end = Tokens.Space.S2, bottom = Tokens.Space.S2)) {
                for (entry in entries) {
                    val word = when (entry.status) {
                        PlanStatus.Done -> "Done"
                        PlanStatus.Running -> "In progress"
                        PlanStatus.Pending -> "Pending"
                    }
                    val tint = when (entry.status) {
                        PlanStatus.Done -> colors.textFaint
                        PlanStatus.Running -> colors.statusWorking
                        PlanStatus.Pending -> colors.textMuted
                    }

                    Row(
                        Modifier.fillMaxWidth().heightIn(min = Tokens.Space.S6).semantics(mergeDescendants = true) { contentDescription = "$word: ${entry.content}" },
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(todoIcon(entry.status), tint, Tokens.Size.IconSm)
                        Label(entry.content, color = if (entry.status == PlanStatus.Running) colors.text else tint, maxLines = 1)
                    }
                }
            }
        }
    }
}

private fun todoIcon(status: PlanStatus): MyIcon = when (status) {
    PlanStatus.Done -> MyIcon.TodoDone
    PlanStatus.Running -> MyIcon.TodoNow
    PlanStatus.Pending -> MyIcon.TodoPending
}
