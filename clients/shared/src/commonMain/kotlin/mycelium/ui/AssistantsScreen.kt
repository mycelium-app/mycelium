package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.Assistant
import mycelium.api.PatchAssistantRequest
import mycelium.state.AllGroups
import mycelium.state.AllStates
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.GroupByGroup
import mycelium.state.GroupByNone
import mycelium.state.GroupByState
import mycelium.state.NoGroup
import mycelium.state.Store
import mycelium.state.assistantRows
import mycelium.state.assistantStateName
import mycelium.state.ago

private val StateChoices = listOf(Choice(AllStates, "All states"), Choice("working", "Working"), Choice("input", "Needs input"), Choice("idle", "Idle"))
private val GroupByChoices = listOf(Choice(GroupByNone, "No grouping"), Choice(GroupByGroup, "Group"), Choice(GroupByState, "State"))

@Composable
internal fun AssistantAvatar(name: String, size: androidx.compose.ui.unit.Dp = Tokens.Size.Avatar) {
    Box(Modifier.size(size).clip(CircleShape).background(MyTheme.colors.bgSelected).clearAndSetSemantics {}, contentAlignment = Alignment.Center) {
        Label(name.trim().take(1).uppercase(), style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
    }
}

@Composable
internal fun AssistantStateMark(state: String) {
    when (state) {
        "working" -> StatusMark(assistantStateName(state), Tone.Working, MyIcon.Working)
        "input" -> StatusMark(assistantStateName(state), Tone.Attention, MyIcon.Alert)
        else -> Label(assistantStateName(state), color = MyTheme.colors.textMuted)
    }
}

internal fun LazyListScope.sidebarAssistantItems(assistants: List<Assistant>, route: Route, onRoute: (Route) -> Unit) {
    item(key = "assistants-label") {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Label("Assistants", Modifier.weight(1f).padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1).semantics { heading() }, Tokens.Type.Meta, MyTheme.colors.textMuted)
            MyButton({ onRoute(Route.Assistants) }, text = "Show all", size = ButtonSize.Sm, muted = true, description = "Show all assistants")
        }
    }

    items(assistants, key = { "assistant-${it.id}" }) { assistant ->
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = rowHeight())
                .pressable({ onRoute(Route.Assistant(assistant.id)) }, description = "${assistant.name}, ${assistantStateName(assistant.state)}", selected = route == Route.Assistant(assistant.id), shape = BlockShape)
                .padding(horizontal = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            AssistantAvatar(assistant.name)
            Label(assistant.name, Modifier.weight(1f), maxLines = 1)

            when (assistant.state) {
                "working" -> Icon(MyIcon.Working, MyTheme.colors.statusWorking, Tokens.Size.IconSm)
                "input" -> Icon(MyIcon.Alert, MyTheme.colors.statusAttention, Tokens.Size.IconSm)
                else -> assistant.lastActiveAt?.let { Label(ago(it), style = Tokens.Type.Meta, color = MyTheme.colors.textFaint, maxLines = 1) }
            }
        }
    }
}

@Composable
fun AssistantsScreen(store: Store, state: AppState, onOpen: (Assistant) -> Unit, onSettings: (Assistant) -> Unit, onNew: () -> Unit) {
    val colors = MyTheme.colors
    val sidebarGroup = LocalGroupFilter.current
    var query by remember { mutableStateOf("") }
    var group by remember(sidebarGroup) { mutableStateOf(sidebarGroup) }
    var showing by remember { mutableStateOf(AllStates) }
    var groupBy by remember { mutableStateOf(GroupByNone) }

    LaunchedEffect(Unit) {
        store.act { store.refreshAssistants() }
    }

    val groupChoices = listOf(Choice(AllGroups, "All groups")) + state.groups.values.sortedBy { it.name.lowercase() }.map { Choice(it.id, it.name) } + Choice(NoGroup, "No group")
    val sections = assistantRows(state.assistants.values, state.groups, query, group, showing, groupBy)

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Assistants")
            Box(Modifier.weight(1f))
            MyButton(onNew, text = "New assistant", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                TextInput(query, { query = it }, label = "Search assistants", placeholder = "Search assistants", showLabel = false, modifier = Modifier.weight(1f))
                OptionMenu("Group", groupChoices, group, { group = it }, title = "Show", icon = MyIcon.Layers)
                OptionMenu("State", StateChoices, showing, { showing = it }, title = "State")
                OptionMenu("Group by", GroupByChoices, groupBy, { groupBy = it }, title = "Group by")
            }

            if (sections.isEmpty()) {
                Label(
                    if (state.assistants.isEmpty()) "Assistants keep their own instructions, files and secrets, and one thread you can come back to." else "No assistant matches.",
                    color = colors.textMuted,
                )

                if (state.assistants.isEmpty()) {
                    MyButton(onNew, text = "New assistant", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
                }
            }

            for (section in sections) {
                section.title?.let { Label(it, Modifier.semantics { heading() }, Tokens.Type.UiStrong, colors.textMuted) }

                val assistants = section.rows

                Table(
                    listOf(
                        Col("Name", 140.dp, rigid = true),
                        Col("Description", 120.dp, weight = 1f, priority = 1),
                        Col("State", 110.dp, preferred = 110.dp),
                        Col("Wakes", 88.dp, max = 160.dp, priority = 1),
                        Col("Last active", 88.dp, max = 120.dp),
                        Col("", 24.dp, preferred = 24.dp),
                        ActionColumn,
                    ),
                    assistants.size,
                    { row, column ->
                        val assistant = assistants[row]

                        when (column) {
                            0 -> TableCell.Slot(assistant.name, Tokens.Space.S8) {
                                Row(
                                    Modifier.pressable({ onOpen(assistant) }, role = Role.Button, description = "Open ${assistant.name}", shape = BlockShape).padding(vertical = Tokens.Space.S1),
                                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    AssistantAvatar(assistant.name)
                                    Label(assistant.name, maxLines = 1)
                                }
                            }

                            1 -> TableCell.Text(assistant.description.orEmpty(), color = colors.textMuted)
                            2 -> TableCell.Slot { AssistantStateMark(assistant.state) }
                            3 -> TableCell.Text(assistant.wakes ?: "Never", color = colors.textMuted)
                            4 -> TableCell.Text(assistant.lastActiveAt?.let { ago(it) } ?: "Never", color = colors.textMuted)
                            5 -> TableCell.Slot {
                                if (assistant.pinnedAt != null) {
                                    Icon(MyIcon.Pin, colors.textMuted, Tokens.Size.IconSm)
                                }
                            }

                            else -> TableCell.Slot {
                                RowMenu(assistant.name) { close ->
                                    MenuItem(if (assistant.pinnedAt == null) "Pin" else "Unpin", onClick = { close(); store.act { store.patchAssistant(assistant, PatchAssistantRequest(pinned = assistant.pinnedAt == null)) } }, leading = MyIcon.Pin)
                                    MenuItem("Settings", onClick = { close(); onSettings(assistant) }, leading = MyIcon.Sliders)
                                    MenuItem("Archive", onClick = { close(); store.act { store.patchAssistant(assistant, PatchAssistantRequest(archived = true)) } }, leading = MyIcon.Archive)
                                }
                            }
                        }
                    },
                    lazy = false,
                )
            }
        }
    }
}
