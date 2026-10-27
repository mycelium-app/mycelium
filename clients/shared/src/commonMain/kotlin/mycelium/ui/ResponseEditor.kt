package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.isShiftPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.ApiException
import mycelium.api.ResponseFilterResult
import mycelium.api.ResponseToEdit
import mycelium.state.ChatToolName
import mycelium.state.KeepState
import mycelium.state.ResponseDraft
import mycelium.state.ResponsePath
import mycelium.state.ResponseShape
import mycelium.state.ThreadItem
import mycelium.state.childSteps
import mycelium.state.itemCount
import mycelium.state.parseEdited
import mycelium.state.pathFrom
import mycelium.state.pathLabel
import mycelium.state.valueText

class ResponseActions(
    val load: suspend (requestId: String) -> ResponseToEdit?,
    val filter: suspend (requestId: String, filter: String) -> ResponseFilterResult,
    val send: suspend (requestId: String, send: JsonElement) -> Unit,
)

val LocalResponseActions = compositionLocalOf<ResponseActions?> { null }

private const val FilterDelayMillis = 300L
private const val NothingSent = "nothing"
private const val SentEdited = "edited"
private const val IndentStep = 20
private const val MinColumnWeight = 6
private const val MaxColumnWeight = 40
private val RowTickWidth = 28.dp

private fun resolvedText(outcome: String?) = when (outcome) {
    NothingSent -> "Nothing sent to the agent"
    SentEdited -> "Sent to the agent, edited"
    else -> "Sent to the agent"
}

private fun draftFrom(loaded: ResponseToEdit): ResponseDraft {
    val draft = ResponseDraft(loaded.value)
    val paths = loaded.selectedPaths ?: return draft

    return draft.select(paths.map { pathFrom(it) })
}

private fun draftFrom(original: JsonElement, result: ResponseFilterResult): ResponseDraft {
    val paths = result.selectedPaths

    if (paths != null) {
        return ResponseDraft(original).select(paths.map { pathFrom(it) })
    }

    return ResponseDraft(result.value ?: original)
}

@Composable
internal fun ResponseEditorCard(response: ThreadItem.ResponseEdit) {
    val actions = LocalResponseActions.current
    val colors = MyTheme.colors
    val loaded by produceState<ResponseToEdit?>(null, response.requestId, response.resolved, actions) {
        value = if (response.resolved) null else actions?.load?.invoke(response.requestId)
    }
    val heading = ChatToolName(response.server, response.tool).heading
    val where = listOfNotNull(response.server, response.account).joinToString(" · ")

    Card {
        CardHead {
            Icon(if (response.resolved) MyIcon.Check else MyIcon.Alert, if (response.resolved) colors.textMuted else colors.statusAttention, Tokens.Size.IconSm)
            Label(heading, style = Tokens.Type.UiStrong, maxLines = 1)
            Label(where, Modifier.weight(1f, fill = false), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
            Box(Modifier.weight(1f))

            if (response.resolved) {
                StatusMark(resolvedText(response.outcome), Tone.Muted, MyIcon.Check)
            } else {
                StatusMark("Waiting for you", Tone.Muted, MyIcon.Clock)
            }
        }

        val shown = loaded

        if (!response.resolved && shown == null) {
            Label("This response is no longer waiting for you.", Modifier.padding(Tokens.Space.S3), Tokens.Type.Meta, colors.textMuted)
        }

        if (!response.resolved && shown != null && actions != null) {
            ResponseEditor(response.requestId, shown, actions)
        }
    }
}

@Composable
private fun ResponseEditor(requestId: String, loaded: ResponseToEdit, actions: ResponseActions) {
    val scope = rememberCoroutineScope()
    val original = loaded.value
    var draft by remember(requestId) { mutableStateOf(draftFrom(loaded)) }
    var filterText by remember(requestId) { mutableStateOf(loaded.filter.orEmpty()) }
    var appliedFilter by remember(requestId) { mutableStateOf(loaded.filter.orEmpty()) }
    var filterError by remember(requestId) { mutableStateOf(loaded.filterError) }
    var editing by remember(requestId) { mutableStateOf<ResponsePath?>(null) }
    val fromAgent = loaded.filter != null && filterText == loaded.filter

    LaunchedEffect(filterText) {
        if (filterText == appliedFilter) {
            return@LaunchedEffect
        }

        delay(FilterDelayMillis)

        try {
            draft = draftFrom(original, actions.filter(requestId, filterText))
            filterError = null
        } catch (failure: ApiException) {
            filterError = failure.message
        }

        appliedFilter = filterText
    }

    FilterBar(filterText, { filterText = it }, fromAgent, filterError)
    Hairline()

    when (draft.shape) {
        ResponseShape.Table -> ResponseTable(draft, { draft = it }, { editing = it })
        ResponseShape.Tree -> ResponseTree(draft, { draft = it }, { editing = it })
        ResponseShape.Text -> ResponseText(draft) { draft = draft.edit(emptyList(), JsonPrimitive(it)) }
    }

    Hairline()

    Row(
        Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label("The agent has not seen this response yet.", Modifier.weight(1f), Tokens.Type.Meta, MyTheme.colors.textMuted)
        MyButton({ scope.launch { actions.send(requestId, JsonNull) } }, text = "Send nothing", variant = ButtonVariant.Outline, description = "Send nothing from this response to the agent")
        MyButton({ scope.launch { actions.send(requestId, draft.result()) } }, text = "Send to agent", variant = ButtonVariant.Primary, description = "Send what you kept of this response to the agent")
    }

    val path = editing

    if (path != null) {
        val current = draft.displayValue(path) ?: JsonNull

        EditValueDialog(path, current, draft.valueAt(path) ?: current, { editing = null }) {
            draft = draft.edit(path, it)
            editing = null
        }
    }
}

@Composable
private fun FilterBar(value: String, onChange: (String) -> Unit, fromAgent: Boolean, error: String?) {
    val colors = MyTheme.colors

    Column(Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label("Filter", style = Tokens.Type.Meta, color = colors.textMuted)
            TextInput(value, onChange, "Filter, jq", Modifier.weight(1f), placeholder = "Add a jq filter to preselect", mono = true, showLabel = false)

            if (fromAgent) {
                Label("From the agent", style = Tokens.Type.Meta, color = colors.textMuted)
                MyButton({ onChange("") }, text = "Clear filter", size = ButtonSize.Sm, description = "Clear the filter the agent sent")
            }
        }

        if (error != null) {
            StatusMark(error, Tone.Failed, MyIcon.Failed)
        }
    }
}

@Composable
private fun ResponseText(draft: ResponseDraft, onChange: (String) -> Unit) {
    val text = valueText(draft.displayValue(emptyList()) ?: JsonNull)

    TextArea(text, onChange, "Response text", Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), mono = true, minLines = 6)
}

private fun cellText(value: JsonElement): String = itemCount(value) ?: valueText(value)

private fun columnWeight(draft: ResponseDraft, column: String): Float {
    val longest = draft.rows.maxOfOrNull { row -> ((row as? JsonObject)?.get(column)?.let(::cellText)).orEmpty().length } ?: 0

    return longest.coerceIn(MinColumnWeight, MaxColumnWeight).coerceAtLeast(column.length).toFloat()
}

@Composable
private fun ResponseTable(draft: ResponseDraft, onChange: (ResponseDraft) -> Unit, onEdit: (ResponsePath) -> Unit) {
    val colors = MyTheme.colors
    var anchor by remember { mutableStateOf(0) }
    var expanded by remember { mutableStateOf(setOf<ResponsePath>()) }
    val columns = draft.columns
    val weights = columns.associateWith { columnWeight(draft, it) }

    Row(Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
        MyButton({ onChange(draft.keepAllRows()) }, text = "Select all", variant = ButtonVariant.Outline, size = ButtonSize.Sm, description = "Keep every row")
        MyButton({ onChange(draft.keepNoRows()) }, text = "Select none", variant = ButtonVariant.Outline, size = ButtonSize.Sm, description = "Drop every row")
        Box(Modifier.weight(1f))
        Label("${draft.keptRowCount()} of ${draft.rows.size} kept. Click a row to keep or drop it, Shift-click for a range.", style = Tokens.Type.Meta, color = colors.textMuted)
    }

    Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Box(Modifier.width(RowTickWidth))

        for (column in columns) {
            Label(column, Modifier.weight(weights.getValue(column)), Tokens.Type.Meta, colors.textMuted)
        }
    }

    for (index in draft.rows.indices) {
        val path = listOf<Any>(index)
        val dropped = draft.isDropped(path)
        val row = draft.rows[index] as? JsonObject
        val number = index + 1

        Hairline()

        KeepableRow(
            kept = !dropped,
            description = "Keep row $number",
            onToggle = { range ->
                onChange(if (range) draft.toggleRange(anchor, index) else draft.toggle(path))
                anchor = index
            },
        ) {
            Box(Modifier.width(RowTickWidth).padding(top = Tokens.Space.S1), contentAlignment = Alignment.TopCenter) {
                TickBox(!dropped)
            }

            for (column in columns) {
                val cell = row?.get(column)

                Box(Modifier.weight(weights.getValue(column)).padding(vertical = Tokens.Space.S1)) {
                    if (cell is JsonObject || cell is JsonArray) {
                        val cellPath = path + column
                        val open = cellPath in expanded

                        MyButton(
                            { expanded = if (open) expanded.filterNot { it == cellPath }.toSet() else expanded + setOf(cellPath) },
                            text = itemCount(cell),
                            icon = if (open) MyIcon.ChevronDown else MyIcon.ChevronRight,
                            size = ButtonSize.Sm,
                            description = "${if (open) "Hide" else "Show"} $column of row $number",
                            stateDescription = if (open) "Expanded" else "Collapsed",
                            muted = true,
                        )
                    } else if (cell != null) {
                        Label(valueText(cell), style = Tokens.Type.Ui, color = if (dropped) colors.textFaint else colors.text)
                    }
                }
            }
        }

        for (cellPath in expanded.filter { it.first() == index && it.size == 2 }) {
            val nested = draft.valueAt(cellPath)

            if (nested != null) {
                Column(Modifier.padding(start = RowTickWidth + Tokens.Space.S2)) {
                    TreeNodes(draft, nested, cellPath, 1, onChange, onEdit, openAtStart = true)
                }
            }
        }
    }
}

@Composable
private fun TickBox(kept: Boolean) {
    val colors = MyTheme.colors

    Box(
        Modifier
            .size(Tokens.Size.IconMd)
            .background(if (kept) colors.accent else colors.bgContent, ControlShape)
            .border(Tokens.Size.Hairline, if (kept) colors.accent else colors.borderControl, ControlShape),
        contentAlignment = Alignment.Center,
    ) {
        if (kept) {
            Icon(MyIcon.Check, colors.onAccent, Tokens.Size.IconSm)
        }
    }
}

@Composable
private fun KeepableRow(kept: Boolean, description: String, onToggle: (range: Boolean) -> Unit, content: @Composable RowScope.() -> Unit) {
    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()

    Row(
        Modifier
            .fillMaxWidth()
            .focusRing(focused, ControlShape, inside = true)
            .focusable(interactionSource = source)
            .onKeyEvent { event ->
                val activates = event.type == KeyEventType.KeyUp && (event.key == Key.Spacebar || event.key == Key.Enter)

                if (activates) {
                    onToggle(false)
                }

                activates
            }
            .pointerInput(Unit) {
                awaitEachGesture {
                    awaitFirstDown()
                    val range = currentEvent.keyboardModifiers.isShiftPressed
                    val up = waitForUpOrCancellation()

                    if (up != null) {
                        up.consume()
                        onToggle(range)
                    }
                }
            }
            .semantics(mergeDescendants = true) {
                role = Role.Checkbox
                contentDescription = description
                stateDescription = if (kept) "Kept" else "Dropped"
                onClick { onToggle(false); true }
            }
            .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S0_5),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        content()
    }
}

@Composable
private fun ResponseTree(draft: ResponseDraft, onChange: (ResponseDraft) -> Unit, onEdit: (ResponsePath) -> Unit) {
    Column(Modifier.padding(vertical = Tokens.Space.S2, horizontal = Tokens.Space.S2).semantics { contentDescription = "Response" }) {
        TreeNodes(draft, draft.base, emptyList(), 0, onChange, onEdit, openAtStart = false)
    }
}

@Composable
private fun TreeNodes(draft: ResponseDraft, node: JsonElement, path: ResponsePath, depth: Int, onChange: (ResponseDraft) -> Unit, onEdit: (ResponsePath) -> Unit, openAtStart: Boolean) {
    for (step in childSteps(node)) {
        val value = when {
            node is JsonObject && step is String -> node[step]
            node is JsonArray && step is Int -> node.getOrNull(step)
            else -> null
        } ?: continue

        TreeRow(draft, value, path + step, step.toString(), depth, onChange, onEdit, openAtStart || depth == 0)
    }
}

@Composable
private fun TreeRow(draft: ResponseDraft, value: JsonElement, path: ResponsePath, key: String, depth: Int, onChange: (ResponseDraft) -> Unit, onEdit: (ResponsePath) -> Unit, openAtStart: Boolean) {
    val colors = MyTheme.colors
    val container = value is JsonObject || value is JsonArray
    var open by remember(path) { mutableStateOf(openAtStart) }
    val state = draft.keepState(path)
    val dropped = draft.isDropped(path)
    val edited = draft.isEdited(path)
    val shown = draft.displayValue(path) ?: value
    val strike = if (dropped) TextDecoration.LineThrough else TextDecoration.None
    val label = pathLabel(path)

    Row(Modifier.fillMaxWidth().padding(start = (depth * IndentStep).dp), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(touchMin(Tokens.Size.ControlMd)), contentAlignment = Alignment.Center) {
            if (container) {
                MyButton(
                    { open = !open },
                    icon = if (open) MyIcon.ChevronDown else MyIcon.ChevronRight,
                    size = ButtonSize.Sm,
                    muted = true,
                    description = "${if (open) "Collapse" else "Expand"} $label",
                    stateDescription = if (open) "Expanded" else "Collapsed",
                )
            }
        }

        Checkbox(
            checked = state == KeepState.All,
            partly = state == KeepState.Some,
            description = "Keep $label",
            onToggle = { onChange(draft.toggle(path)) },
            checkedState = "Kept",
            uncheckedState = "Dropped",
        )

        Label(key, style = Tokens.Type.Code.copy(textDecoration = strike), color = if (dropped) colors.textFaint else colors.textMuted)

        Label(
            if (container) itemCount(shown).orEmpty().let { if (value is JsonArray) "[$it]" else "{$it}" } else quoted(shown),
            Modifier.weight(1f),
            Tokens.Type.Code.copy(textDecoration = strike),
            if (dropped) colors.textFaint else colors.text,
        )

        if (edited) {
            Tag("Edited")
            MyButton({ onChange(draft.reset(path)) }, text = "Reset", size = ButtonSize.Sm, muted = true, description = "Reset $label to the original value")
        }

        if (!container) {
            MyButton({ onEdit(path) }, icon = MyIcon.Pencil, size = ButtonSize.Sm, muted = true, description = "Edit $label")
        }
    }

    if (container && open) {
        TreeNodes(draft, value, path, depth + 1, onChange, onEdit, openAtStart = false)
    }
}

private fun quoted(value: JsonElement): String = if (value is JsonPrimitive && value.isString) "\"${value.content}\"" else value.toString()

@Composable
private fun EditValueDialog(path: ResponsePath, current: JsonElement, original: JsonElement, onDismiss: () -> Unit, onSave: (JsonElement) -> Unit) {
    var text by remember(path) { mutableStateOf(valueText(current)) }
    var problem by remember(path) { mutableStateOf<String?>(null) }
    val label = pathLabel(path)
    val isText = original is JsonPrimitive && original.isString

    fun save() {
        val parsed = parseEdited(text, original)

        if (parsed == null) {
            problem = "Enter a number, true, false or null for $label."

            return
        }

        onSave(parsed)
    }

    Dialog(onDismiss) {
        Label("Edit $label", Modifier.semantics { heading() }, Tokens.Type.Title)
        TextArea(text, { text = it; problem = null }, "Value", mono = true, minLines = 4, error = problem)
        Label(if (isText) "The agent gets this text in place of the original." else "The agent gets this value in place of the original.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Primary)
        }
    }
}
