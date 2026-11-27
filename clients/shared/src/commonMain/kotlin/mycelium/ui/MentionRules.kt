package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import kotlinx.coroutines.delay
import mycelium.api.ChatToolServer
import mycelium.api.MentionPerson
import mycelium.api.MentionPlace
import mycelium.api.SaveMentionTriggerRequest
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.HarnessAgents
import mycelium.state.agentName
import mycelium.state.Store
import mycelium.state.editable
import mycelium.state.mentionFrom
import mycelium.state.mentionIn
import mycelium.state.mentionRules
import mycelium.state.mentionTarget
import mycelium.state.mentionTargetName
import mycelium.state.mentionsHint
import mycelium.state.withMentionTarget

private const val Draft = "draft"
private const val AnyServer = "any"
private const val PeopleSearchDelayMillis = 250L
private val NumberColumn = 28.dp
private val GripColumn = 24.dp

@Composable
internal fun MentionRules(store: Store, live: AppState, integration: String, accounts: List<ChatToolServer>) {
    val colors = MyTheme.colors
    val rules = mentionRules(live.mentionTriggers, integration)
    var open by remember(integration) { mutableStateOf<String?>(null) }
    var draft by remember(integration) { mutableStateOf<SaveMentionTriggerRequest?>(null) }
    var places by remember(integration) { mutableStateOf<List<MentionPlace>>(emptyList()) }
    var failure by remember(integration) { mutableStateOf<String?>(null) }
    var dragging by remember { mutableStateOf<String?>(null) }
    var dragOffset by remember { mutableStateOf(0f) }
    val heights = remember { mutableStateMapOf<String, Int>() }
    val shown = rules.map { it.id } + listOfNotNull(Draft.takeIf { open == Draft })

    LaunchedEffect(integration) {
        runCatching { store.api.mentionPlaces(integration) }.onSuccess { places = it }
    }

    fun dropIndex(from: Int): Int {
        var index = from
        var travelled = dragOffset

        while (travelled > 0 && index < rules.lastIndex && travelled > (heights[rules[index + 1].id] ?: 0) / 2f) {
            travelled -= heights[rules[index + 1].id] ?: 0
            index++
        }

        while (travelled < 0 && index > 0 && -travelled > (heights[rules[index - 1].id] ?: 0) / 2f) {
            travelled += heights[rules[index - 1].id] ?: 0
            index--
        }

        return index
    }

    fun close() {
        open = null
        draft = null
        failure = null
    }

    fun openRule(id: String, rule: SaveMentionTriggerRequest) {
        open = id
        draft = rule
        failure = null
    }

    fun move(id: String, to: Int) {
        store.act { store.moveMentionRule(integration, id, to) }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Label("Mentions", Modifier.weight(1f).semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)
            MyButton({ openRule(Draft, SaveMentionTriggerRequest(integration)) }, text = "Add rule", icon = MyIcon.Plus, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        Label(mentionsHint(integration), style = Tokens.Type.Meta, color = colors.textMuted)

        Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, ControlShape).semantics { contentDescription = "Mention rules" }) {
            RuleCells(Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5), "#", "In", "From", "Goes to", muted = true)

            val from = rules.indexOfFirst { it.id == dragging }
            val target = if (from >= 0) dropIndex(from) else -1

            for ((index, id) in shown.withIndex()) {
                val saved = rules.firstOrNull { it.id == id }
                val rule = if (open == id) draft ?: SaveMentionTriggerRequest(integration) else saved?.let(::editable) ?: continue
                val currentIndex by rememberUpdatedState(index)
                val number = index + 1
                val name = mentionTargetName(rule, live.assistants, live.projects) ?: "Choose where it goes"
                val shift = when {
                    id == dragging -> dragOffset
                    from in 0 until index && index <= target -> -(heights[rules[from].id] ?: 0).toFloat()
                    from > index && index >= target && target >= 0 -> (heights[rules[from].id] ?: 0).toFloat()
                    else -> 0f
                }

                Hairline()

                Column(
                    Modifier
                        .fillMaxWidth()
                        .zIndex(if (id == dragging) 1f else 0f)
                        .graphicsLayer { translationY = shift }
                        .onSizeChanged { heights[id] = it.height }
                        .background(if (open == id) colors.bgPanel else androidx.compose.ui.graphics.Color.Transparent),
                ) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = Tokens.Size.Row)
                            .pressable({ if (open == id) close() else openRule(id, rule) }, description = "Rule $number: ${mentionIn(rule, accounts)}, ${mentionFrom(rule)}, goes to $name", stateDescription = if (open == id) "Open" else null)
                            .padding(horizontal = Tokens.Space.S2),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Label("$number", Modifier.width(NumberColumn), style = Tokens.Type.Meta, color = colors.textMuted)

                        Box(
                            Modifier
                                .size(GripColumn)
                                .clearAndSetSemantics {}
                                .pointerInput(id) {
                                    detectDragGestures(
                                        onDragStart = {
                                            dragging = id
                                            dragOffset = 0f
                                        },
                                        onDrag = { change, amount ->
                                            change.consume()
                                            dragOffset += amount.y
                                        },
                                        onDragEnd = {
                                            val dropped = dropIndex(currentIndex)

                                            dragging = null
                                            dragOffset = 0f

                                            if (dropped != currentIndex) {
                                                move(id, dropped)
                                            }
                                        },
                                        onDragCancel = {
                                            dragging = null
                                            dragOffset = 0f
                                        },
                                    )
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            if (saved != null) {
                                Icon(MyIcon.Grip, colors.textFaint, Tokens.Size.IconSm)
                            }
                        }

                        RuleCells(Modifier.weight(1f), null, mentionIn(rule, accounts), mentionFrom(rule), name, muted = false)

                        if (open == id) {
                            Icon(MyIcon.ChevronUp, colors.textMuted, Tokens.Size.IconSm, Modifier.padding(horizontal = Tokens.Space.S1_5))
                        } else {
                            RowMenu("rule $number") { closeMenu ->
                                MenuItem("Edit", onClick = { closeMenu(); openRule(id, rule) }, leading = MyIcon.Pencil)

                                if (index > 0) {
                                    MenuItem("Move up", onClick = { closeMenu(); move(id, index - 1) }, leading = MyIcon.ArrowUp)
                                }

                                if (index < rules.lastIndex) {
                                    MenuItem("Move down", onClick = { closeMenu(); move(id, index + 1) }, leading = MyIcon.ArrowDown)
                                }

                                Hairline(Modifier.padding(vertical = Tokens.Space.S1))
                                MenuItem("Remove rule", onClick = { closeMenu(); store.act { store.removeMentionRule(id) } }, leading = MyIcon.Close)
                            }
                        }
                    }

                    if (open == id) {
                        RuleEditor(
                            store,
                            live,
                            rule,
                            places,
                            accounts,
                            failure,
                            onChange = { draft = it },
                            onRemove = {
                                if (saved != null) {
                                    store.act { store.removeMentionRule(id) }
                                }

                                close()
                            },
                            onDone = {
                                if (mentionTarget(rule) == null) {
                                    failure = "Choose where the messages go: an assistant, or an agent in a project"
                                } else {
                                    store.act {
                                        if (saved == null) store.addMentionRule(rule) else store.saveMentionRule(id, rule)
                                        close()
                                    }
                                }
                            },
                        )
                    }
                }
            }

            Hairline()
            Row(Modifier.fillMaxWidth().heightIn(min = Tokens.Size.Row).padding(horizontal = Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.width(NumberColumn + GripColumn))
                RuleCells(Modifier.weight(1f), null, "Anything else", "", "Nobody", muted = true)
                Box(Modifier.width(Tokens.Size.ControlSm))
            }
        }
    }
}

@Composable
private fun RuleCells(modifier: Modifier, number: String?, place: String, from: String, goesTo: String, muted: Boolean) {
    val colors = MyTheme.colors
    val style = if (muted) Tokens.Type.Meta else Tokens.Type.Ui
    val colour = if (muted) colors.textMuted else colors.text

    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        if (number != null) {
            Label(number, Modifier.width(NumberColumn + GripColumn - Tokens.Space.S2), style = style, color = colour)
        }

        Label(place, Modifier.weight(1f), style = style, color = colour, maxLines = 1)
        Label(from, Modifier.weight(1f), style = style, color = colour, maxLines = 1)
        Label(goesTo, Modifier.weight(1f), style = style, color = colour, maxLines = 1)

        if (number != null) {
            Box(Modifier.width(Tokens.Size.ControlSm))
        }
    }
}

@Composable
private fun RuleEditor(
    store: Store,
    live: AppState,
    rule: SaveMentionTriggerRequest,
    places: List<MentionPlace>,
    accounts: List<ChatToolServer>,
    failure: String?,
    onChange: (SaveMentionTriggerRequest) -> Unit,
    onRemove: () -> Unit,
    onDone: () -> Unit,
) {
    val colors = MyTheme.colors
    val kept = rule.serverId?.takeIf { rule.guildId == null }?.let { "account:$it" }
    val placeChoices = listOf(Choice(AnyServer, "Any server")) +
        listOfNotNull(kept?.let { Choice(it, mentionIn(rule, accounts)) }) +
        places.map { Choice(it.id, it.name) } +
        listOfNotNull(rule.guildId?.takeIf { guild -> places.none { it.id == guild } }?.let { Choice(it, rule.guildName ?: it) })
    val targets = live.assistants.values.filter { it.archivedAt == null }.sortedBy { it.name }.map { Choice("assistant:${it.id}", it.name, group = "Assistants") } +
        live.projects.values.sortedBy { it.name }.flatMap { project -> HarnessAgents.map { agent -> Choice("agent:${project.id}:$agent", "${agentName(agent)} in ${project.name}", group = "Agents") } }

    Column(Modifier.fillMaxWidth().padding(start = Tokens.Space.S8, end = Tokens.Space.S2, bottom = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        EditorLine("In", "Server, or Any server") {
            OptionMenu(
                "In",
                placeChoices,
                rule.guildId ?: kept ?: AnyServer,
                { picked ->
                    onChange(
                        when {
                            picked == AnyServer -> rule.copy(serverId = null, guildId = null, guildName = null)
                            picked == kept -> rule
                            else -> rule.copy(serverId = null, guildId = picked, guildName = placeChoices.first { it.id == picked }.name)
                        },
                    )
                },
                variant = ButtonVariant.Outline,
                size = ButtonSize.Sm,
            )
        }

        EditorLine("From", "Empty means everyone") {
            PeoplePicker(store, rule, onChange)
        }

        EditorLine("Goes to", "An assistant, or an agent in a project") {
            OptionMenu("Goes to", targets, mentionTarget(rule), { onChange(withMentionTarget(rule, it)) }, label = "Choose", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        if (rule.channels.isNotEmpty()) {
            EditorLine("Channels", "Only these channels") {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    Label(rule.channels.joinToString(", "), style = Tokens.Type.Code, maxLines = 1)
                    MyButton({ onChange(rule.copy(channels = emptyList())) }, text = "Any channel", size = ButtonSize.Sm, muted = true)
                }
            }
        }

        failure?.let { Label(it, style = Tokens.Type.Meta, color = colors.statusFailed, modifier = Modifier.semantics { contentDescription = "Error: $it" }) }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
            MyButton(onRemove, text = "Remove rule", variant = ButtonVariant.Danger, size = ButtonSize.Sm)
            MyButton(onDone, text = "Done", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }
    }
}

@Composable
private fun EditorLine(name: String, hint: String, control: @Composable () -> Unit) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
        Label(name, Modifier.width(72.dp), style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        Box(Modifier.weight(1f)) { control() }
        Label(hint, Modifier.weight(1f), style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
    }
}

@Composable
private fun PeoplePicker(store: Store, rule: SaveMentionTriggerRequest, onChange: (SaveMentionTriggerRequest) -> Unit) {
    var query by remember { mutableStateOf("") }
    var found by remember { mutableStateOf<List<MentionPerson>>(emptyList()) }

    LaunchedEffect(query, rule.guildId) {
        found = emptyList()

        if (query.isNotBlank()) {
            delay(PeopleSearchDelayMillis)
            runCatching { store.api.mentionPeople(rule.integration, rule.guildId, query.trim()) }.onSuccess { people -> found = people.filter { it.id !in rule.senders } }
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            for ((index, id) in rule.senders.withIndex()) {
                val name = rule.senderNames.getOrNull(index)?.ifBlank { null } ?: id

                MyButton(
                    { onChange(rule.copy(senders = rule.senders.filterIndexed { at, _ -> at != index }, senderNames = rule.senderNames.filterIndexed { at, _ -> at != index })) },
                    text = name,
                    trailingIcon = MyIcon.Close,
                    description = "Remove $name",
                    variant = ButtonVariant.Filled,
                    size = ButtonSize.Sm,
                )
            }
        }

        TextInput(query, { query = it }, label = "Add a person", placeholder = "Add a person…", showLabel = false)

        for (person in found) {
            MenuItem(person.name, onClick = {
                onChange(rule.copy(senders = rule.senders + person.id, senderNames = rule.senderNames.take(rule.senders.size) + person.name))
                query = ""
            }, leading = MyIcon.Plus)
        }
    }
}
