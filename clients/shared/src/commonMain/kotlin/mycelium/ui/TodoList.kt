package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.Role
import mycelium.state.PlanEntry
import mycelium.state.PlanStatus
import mycelium.state.todoCurrentIndex

@Composable
fun GoalAndTodoRegion(goal: GoalControls?, todos: List<PlanEntry>, conversationId: String, modifier: Modifier = Modifier, onSaveTodos: ((List<PlanEntry>, Boolean) -> Unit)? = null) {
    val held = goal?.goal
    val label = when {
        held != null && todos.isNotEmpty() -> "Goal and to-do list"
        held != null -> "Goal"
        else -> "To-do list"
    }

    Column(modifier.fillMaxWidth().semantics { contentDescription = label }) {
        goal?.goal?.let { GoalBar(it, goal) }

        if (todos.isNotEmpty()) {
            TodoList(todos, conversationId, onSave = onSaveTodos)
        }
    }
}

private data class DraftItem(val origin: PlanEntry?, val content: String, val description: String, val status: PlanStatus)

@Composable
fun TodoList(entries: List<PlanEntry>, conversationId: String, modifier: Modifier = Modifier, onSave: ((List<PlanEntry>, Boolean) -> Unit)? = null) {
    val colors = MyTheme.colors
    var expanded by remember(conversationId) { mutableStateOf(false) }
    var opened by remember(conversationId) { mutableStateOf(emptySet<String>()) }
    val draft = remember(conversationId) { mutableStateListOf<DraftItem>() }
    var editing by remember(conversationId) { mutableStateOf(false) }
    var selected by remember(conversationId) { mutableStateOf(-1) }
    var checked by remember(conversationId) { mutableStateOf(false) }
    val currentIndex = todoCurrentIndex(entries)
    val current = entries[currentIndex]
    val count = "${currentIndex + 1} of ${entries.size}"

    fun startEditing() {
        draft.clear()
        draft.addAll(entries.map { DraftItem(it, it.content, it.description.orEmpty(), it.status) })
        selected = -1
        checked = false
        editing = true
    }

    fun save(tellAgent: Boolean) {
        checked = true

        if (draft.any { it.content.isBlank() }) return

        onSave?.invoke(draft.map { PlanEntry(it.content.trim(), it.status, it.description.trim().ifEmpty { null }) }, tellAgent)
        editing = false
    }

    Column(modifier.fillMaxWidth()) {
        if (editing) {
            val kept = draft.mapNotNull { it.origin }
            val changes = draft.count { it.origin == null || it.content != it.origin.content || it.description != it.origin.description.orEmpty() } + entries.count { it !in kept }

            Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(start = Tokens.Space.S3, end = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Icon(MyIcon.Pencil, colors.textMuted, Tokens.Size.IconSm)
                Label("Editing to-do", Modifier.weight(1f), color = colors.textMuted, maxLines = 1)

                if (changes > 0) {
                    Label(if (changes == 1) "1 change" else "$changes changes", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }
            }

            Column(Modifier.fillMaxWidth().padding(start = Tokens.Space.S3, end = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                for ((index, item) in draft.withIndex()) {
                    val number = index + 1

                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
                        Icon(todoIcon(item.status), colors.textMuted, Tokens.Size.IconSm, Modifier.padding(top = (touchMin(Tokens.Size.ControlMd) - Tokens.Size.IconSm) / 2))

                        Column(Modifier.weight(1f).onFocusChanged { if (it.hasFocus) selected = index }, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                            TextInput(
                                item.content,
                                { text -> draft[index] = item.copy(content = text) },
                                "Item $number",
                                showLabel = false,
                                error = if (checked && item.content.isBlank()) "Enter what to do, or dismiss item $number" else null,
                            )

                            if (selected == index) {
                                TextInput(item.description, { text -> draft[index] = item.copy(description = text) }, "Details of item $number", showLabel = false, placeholder = "Add details for the agent")
                            }
                        }

                        MyButton({ draft.removeAt(index); selected = -1 }, icon = MyIcon.Close, description = "Dismiss item $number", size = ButtonSize.Sm, muted = true)
                    }
                }
            }

            Row(Modifier.fillMaxWidth().padding(start = Tokens.Space.S3, end = Tokens.Space.S2, top = Tokens.Space.S2, bottom = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                MyButton({ draft.add(DraftItem(null, "", "", PlanStatus.Pending)); selected = draft.lastIndex }, text = "Add item", icon = MyIcon.Plus, size = ButtonSize.Sm, muted = true)
                Spacer(Modifier.weight(1f))
                MyButton({ editing = false }, text = "Cancel", size = ButtonSize.Sm, muted = true)
                SplitButton("Save", { save(true) }, listOf(SplitAlternative("Save without telling the agent", { save(false) })), size = ButtonSize.Sm, description = "Save and tell the agent")
            }

            return@Column
        }

        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Row(
                Modifier
                    .weight(1f)
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

            if (expanded && onSave != null) {
                MyButton(::startEditing, Modifier.padding(end = Tokens.Space.S2), text = "Edit", icon = MyIcon.Pencil, description = "Edit the to-do list", size = ButtonSize.Sm, muted = true)
            }
        }

        if (expanded) {
            Column(Modifier.fillMaxWidth().padding(start = Tokens.Space.S3, end = Tokens.Space.S2, bottom = Tokens.Space.S2)) {
                for ((index, entry) in entries.withIndex()) {
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
                    val description = entry.description
                    val open = description != null && entry.content in opened
                    val step = if (index == currentIndex) "Current step" else null
                    val row = Modifier.fillMaxWidth().heightIn(min = Tokens.Space.S6)
                    val target = when (description) {
                        null -> row.semantics(mergeDescendants = true) {
                            contentDescription = "$word: ${entry.content}"

                            if (step != null) {
                                stateDescription = step
                            }
                        }
                        else -> row.pressable(
                            { opened = if (open) opened - entry.content else opened + entry.content },
                            role = Role.Button,
                            description = "$word: ${entry.content}",
                            shape = ControlShape,
                            stateDescription = listOfNotNull(step, if (open) "Expanded" else "Collapsed").joinToString(", "),
                        )
                    }

                    Row(target, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                        Icon(todoIcon(entry.status), tint, Tokens.Size.IconSm)
                        Label(entry.content, Modifier.weight(1f), color = if (entry.status == PlanStatus.Running) colors.text else tint, maxLines = 1)

                        if (description != null) {
                            Icon(if (open) MyIcon.ChevronDown else MyIcon.ChevronRight, colors.textFaint, Tokens.Size.IconSm)
                        }
                    }

                    if (open && description != null) {
                        Label(description, Modifier.padding(start = Tokens.Size.IconSm + Tokens.Space.S2, bottom = Tokens.Space.S1), style = Tokens.Type.Meta, color = colors.textMuted)
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
