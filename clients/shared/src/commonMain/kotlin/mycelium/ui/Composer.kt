package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import mycelium.api.Conversation
import mycelium.api.QueuedPrompt

data class AgentMode(val id: String, val label: String)

fun modesFor(agent: String): List<AgentMode> = when (agent) {
    "claude" -> listOf(
        AgentMode("default", "Default"),
        AgentMode("plan", "Plan"),
        AgentMode("acceptEdits", "Auto"),
        AgentMode("bypassPermissions", "Bypass"),
    )
    "codex" -> listOf(
        AgentMode("read-only", "Default"),
        AgentMode("auto", "Auto"),
        AgentMode("full-access", "Bypass"),
    )
    else -> emptyList()
}

@Composable
private fun modeColor(label: String): Color {
    val colors = MyTheme.colors

    return when (label) {
        "Plan" -> colors.statusWorking
        "Auto" -> colors.statusAttention
        "Bypass" -> colors.statusFailed
        else -> colors.textMuted
    }
}

@Composable
fun Composer(
    conversation: Conversation,
    working: Boolean,
    queue: List<QueuedPrompt>,
    onSend: (String) -> Unit,
    onStop: () -> Unit,
    onSendQueuedNow: (String) -> Unit,
    onEditQueued: (String, String) -> Unit,
    onRemoveQueued: (String) -> Unit,
    onSetMode: (String) -> Unit,
    onSetModel: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = MyTheme.colors
    var text by remember(conversation.id) { mutableStateOf("") }
    val source = remember { MutableInteractionSource() }
    val focused by source.collectIsFocusedAsState()
    val modes = modesFor(conversation.agent)
    val currentMode = modes.firstOrNull { it.id == conversation.mode }

    fun submit() {
        val trimmed = text.trim()

        if (trimmed.isNotEmpty()) {
            onSend(trimmed)
            text = ""
        }
    }

    if (LocalTouch.current) {
        val shape = RoundedCornerShape(Tokens.Radius.Card)

        Column(modifier.fillMaxWidth().padding(start = Tokens.Space.S2, end = Tokens.Space.S2, bottom = Tokens.Space.S2)) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .shadow(Tokens.Space.S2, shape, ambientColor = colors.shadowFloat, spotColor = colors.shadowFloat)
                    .background(colors.bgRaised, shape)
                    .border(Tokens.Size.Hairline, if (focused) colors.focus else colors.border, shape)
                    .padding(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S1),
            ) {
                if (queue.isNotEmpty()) {
                    QueuedMessages(queue, onSendQueuedNow, onEditQueued, onRemoveQueued)
                }

                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    MessageField(text, { text = it }, source, ::submit, Modifier.weight(1f).padding(start = Tokens.Space.S2))
                    SendButtons(working, onStop, ::submit)
                }

                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                    ModelMenu(conversation.model, onSetModel)

                    if (modes.isNotEmpty()) {
                        ModeMenu(modes, currentMode, onSetMode)
                    }
                }
            }
        }

        return
    }

    Column(
        modifier
            .fillMaxWidth()
            .background(colors.bgContent)
            .drawBehind {
                val line = Tokens.Size.Hairline.toPx()
                drawRect(if (focused) colors.focus else colors.border, Offset.Zero, androidx.compose.ui.geometry.Size(size.width, line))
            },
    ) {
        if (queue.isNotEmpty()) {
            QueuedMessages(queue, onSendQueuedNow, onEditQueued, onRemoveQueued)
        }

        MessageField(text, { text = it }, source, ::submit, Modifier.padding(start = Tokens.Space.S3, top = Tokens.Space.S2, end = Tokens.Space.S3))

        Row(
            Modifier.fillMaxWidth().padding(start = Tokens.Space.S1, top = Tokens.Space.S1, end = Tokens.Space.S2, bottom = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.weight(1f))

            if (working) {
                Icon(MyIcon.Working, colors.statusWorking, modifier = Modifier.padding(horizontal = Tokens.Space.S1).semantics { contentDescription = "Working" })
            }

            if (modes.isNotEmpty()) {
                ModeMenu(modes, currentMode, onSetMode)
            }

            ModelMenu(conversation.model, onSetModel)
            SendButtons(working, onStop, ::submit)
        }
    }
}

@Composable
private fun MessageField(text: String, onTextChange: (String) -> Unit, source: MutableInteractionSource, onSubmit: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    BasicTextField(
        value = text,
        onValueChange = onTextChange,
        textStyle = Tokens.Type.Prose.themed().copy(color = colors.text),
        cursorBrush = SolidColor(colors.text),
        interactionSource = source,
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = Tokens.Size.ComposerInput)
            .semantics { contentDescription = "Message" }
            .onPreviewKeyEvent { event ->
                val enter = event.key == Key.Enter && !event.isShiftPressed

                if (enter && event.type == KeyEventType.KeyDown) {
                    onSubmit()
                }

                enter
            },
        decorationBox = { inner ->
            Box(contentAlignment = Alignment.CenterStart) {
                if (text.isEmpty()) {
                    Label("Ask, plan or build", style = Tokens.Type.Prose, color = colors.textFaint)
                }

                inner()
            }
        },
    )
}

@Composable
private fun SendButtons(working: Boolean, onStop: () -> Unit, onSend: () -> Unit) {
    if (working) {
        MyButton(onStop, icon = MyIcon.Stop, description = "Stop the agent", variant = ButtonVariant.Outline)
    }

    MyButton(
        onClick = onSend,
        icon = MyIcon.Send,
        description = if (working) "Queue message" else "Send",
        variant = ButtonVariant.Primary,
    )
}

@Composable
private fun ModeMenu(modes: List<AgentMode>, current: AgentMode?, onSelect: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val label = current?.label ?: "Mode"
    val tint = modeColor(label)

    Box {
        Row(
            Modifier.pressable(onClick = { open = true }, description = "Mode: $label", shape = ControlShape)
                .heightIn(min = touchMin(Tokens.Size.ControlMd))
                .padding(horizontal = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(MyIcon.Mode, tint)
            Label(label, style = Tokens.Type.UiStrong, color = tint, maxLines = 1)
        }

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                MenuLabel("Mode")

                for (mode in modes) {
                    MenuItem(mode.label, onClick = { onSelect(mode.id); open = false }, checked = mode.id == current?.id)
                }
            }
        }
    }
}

@Composable
private fun ModelMenu(current: String?, onSelect: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    var draft by remember(current) { mutableStateOf(current.orEmpty()) }

    Box {
        MyButton({ open = true }, text = current ?: "Model", description = "Model: ${current ?: "not set"}", muted = true, trailingIcon = MyIcon.ChevronDown)

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                Column(Modifier.padding(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    TextInput(draft, { draft = it }, label = "Model", placeholder = "Model name", mono = true)

                    MyButton(
                        onClick = {
                            if (draft.isNotBlank()) {
                                onSelect(draft.trim())
                                open = false
                            }
                        },
                        text = "Use this model",
                        variant = ButtonVariant.Primary,
                    )
                }
            }
        }
    }
}

@Composable
private fun QueuedMessages(
    queue: List<QueuedPrompt>,
    onSendNow: (String) -> Unit,
    onEdit: (String, String) -> Unit,
    onRemove: (String) -> Unit,
) {
    val colors = MyTheme.colors
    var editing by remember { mutableStateOf<String?>(null) }
    var draft by remember { mutableStateOf("") }

    Column(Modifier.fillMaxWidth().semantics { contentDescription = "Queued messages" }) {
        for (item in queue) {
            Row(
                Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (editing == item.id) {
                    TextInput(draft, { draft = it }, label = "Queued message", modifier = Modifier.weight(1f), showLabel = false)
                    MyButton({ onEdit(item.id, draft.trim()); editing = null }, text = "Save", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                    MyButton({ editing = null }, text = "Cancel", size = ButtonSize.Sm, muted = true)
                } else {
                    Icon(MyIcon.Clock, colors.textMuted, Tokens.Size.IconSm, Modifier.semantics { contentDescription = "Queued" })
                    Label(item.text, Modifier.weight(1f), maxLines = 1)
                    MyButton({ onSendNow(item.id) }, text = "Send now", size = ButtonSize.Sm, muted = true)
                    MyButton({ draft = item.text; editing = item.id }, icon = MyIcon.Pencil, description = "Edit queued message", size = ButtonSize.Sm, muted = true)
                    MyButton({ onRemove(item.id) }, icon = MyIcon.Close, description = "Remove queued message", size = ButtonSize.Sm, muted = true)
                }
            }

            Hairline()
        }
    }
}
