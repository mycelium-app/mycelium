package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.foundation.interaction.MutableInteractionSource
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
import androidx.compose.runtime.DisposableEffect
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
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import mycelium.state.Choice
import mycelium.state.DefaultAccountId
import mycelium.state.accountMenuTitle
import mycelium.state.Annotation
import mycelium.state.SessionOptions
import mycelium.state.annotationLabel
import mycelium.state.SideChatCommands
import mycelium.state.sideChatOf
import mycelium.state.SlashCommand
import mycelium.state.mentionAt
import kotlinx.coroutines.delay
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import mycelium.api.Conversation
import mycelium.api.QueuedPrompt

fun fallbackModes(agent: String): List<Choice> = when (agent) {
    "claude" -> listOf(
        Choice("default", "Default"),
        Choice("plan", "Plan"),
        Choice("acceptEdits", "Auto"),
        Choice("bypassPermissions", "Bypass"),
    )
    "codex" -> listOf(
        Choice("read-only", "Default"),
        Choice("auto", "Auto"),
        Choice("full-access", "Bypass"),
    )
    else -> emptyList()
}

@Composable
fun Composer(
    conversation: Conversation,
    working: Boolean,
    queue: List<QueuedPrompt>,
    sending: List<String>,
    onSend: (String) -> Unit,
    onStop: () -> Unit,
    onSendQueuedNow: (String) -> Unit,
    onEditQueued: (String, String) -> Unit,
    onRemoveQueued: (String) -> Unit,
    options: SessionOptions,
    commands: List<SlashCommand>,
    onSetMode: (String) -> Unit,
    onSetModel: (String) -> Unit,
    onSetSetting: (String, String) -> Unit,
    onSearchFiles: suspend (String) -> List<String>,
    modifier: Modifier = Modifier,
    accounts: List<Choice> = emptyList(),
    accountLocked: Boolean = false,
    onSetAccount: (String) -> Unit = {},
    onSideChat: (kind: String, text: String) -> Unit = { _, _ -> },
    annotations: List<Annotation> = emptyList(),
    onRemoveAnnotation: (Annotation) -> Unit = {},
    goal: GoalControls? = null,
    attachments: AttachmentControls? = null,
) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    var text by remember(conversation.id) { mutableStateOf(TextFieldValue("")) }
    var sent by remember(conversation.id) { mutableStateOf<String?>(null) }
    val source = remember { MutableInteractionSource() }
    val modes = options.modes.ifEmpty { fallbackModes(conversation.agent) }
    val settings = options.settings.filterNot { (it.category == "mode" && options.modes.isNotEmpty()) || (it.category == "model" && options.models.isNotEmpty()) }
    val query = text.text.takeIf { it.startsWith("/") && it.none(Char::isWhitespace) }?.drop(1)
    val mention = if (query == null) mentionAt(text.text, text.selection.end) else null
    var files by remember { mutableStateOf(emptyList<String>()) }

    LaunchedEffect(mention?.query) {
        files = emptyList()
        val wanted = mention?.query ?: return@LaunchedEffect
        delay(FileSearchDelayMillis)
        files = runCatching { onSearchFiles(wanted) }.getOrDefault(emptyList())
    }

    fun insert(start: Int, completed: String) {
        val updated = text.text.substring(0, start) + completed + text.text.substring(text.selection.end)
        text = TextFieldValue(updated, TextRange(start + completed.length))
    }

    val matches = when {
        query != null -> (SideChatCommands + commands.filterNot { command -> SideChatCommands.any { it.name == command.name } }).filter { it.name.contains(query, ignoreCase = true) }.sortedBy { !it.name.startsWith(query, ignoreCase = true) }
            .map { command -> Completion(command.name, command.description) { insert(0, "/${command.name} ") } }
        mention != null -> files.map { path -> Completion(path, "") { insert(mention.start, "@$path ") } }
        else -> emptyList()
    }
    var highlighted by remember(query, mention?.query) { mutableStateOf(0) }

    DisposableEffect(platform, attachments?.onAdd) {
        val onAdd = attachments?.onAdd

        platform?.listenForFiles(onAdd)

        onDispose { platform?.listenForFiles(null) }
    }

    fun submit() {
        val trimmed = text.text.trim()

        if (trimmed.isNotEmpty() || annotations.isNotEmpty() || attachments?.drafts?.isNotEmpty() == true) {
            val side = sideChatOf(trimmed)

            if (side != null) {
                onSideChat(side.first, side.second)
            } else {
                onSend(trimmed)
            }

            sent = text.text
            text = TextFieldValue("")
        } else if (queue.isNotEmpty()) {
            onSendQueuedNow(queue.minBy { it.position }.id)
        }
    }

    val field: @Composable (Modifier) -> Unit = { fieldModifier ->
        Box(fieldModifier) {
            MessageField(
                text = text,
                onTextChange = { changed ->
                    val stale = sent?.takeIf { it.isNotEmpty() && changed.text.startsWith(it) }
                    sent = null
                    text = if (stale == null) changed else TextFieldValue(changed.text.removePrefix(stale), TextRange(changed.selection.end - stale.length))
                },
                source = source,
                onSubmit = ::submit,
                onPaste = {
                    val pasted = platform?.clipboardFiles().orEmpty()

                    attachments?.onAdd?.invoke(pasted)
                    attachments != null && pasted.isNotEmpty()
                },
                onKey = { key ->
                    when {
                        key == Key.Escape && matches.isEmpty() && working -> { onStop(); true }
                        matches.isEmpty() -> false
                        key == Key.DirectionDown -> { highlighted = (highlighted + 1) % matches.size; true }
                        key == Key.DirectionUp -> { highlighted = (highlighted - 1 + matches.size) % matches.size; true }
                        key == Key.Tab || key == Key.Enter -> { matches[highlighted.coerceIn(matches.indices)].apply(); true }
                        key == Key.Escape -> { text = TextFieldValue(""); true }
                        else -> false
                    }
                },
            )

            if (matches.isNotEmpty()) {
                CompletionPopup(if (query != null) "Commands" else "Files", matches, highlighted.coerceIn(matches.indices))
            }
        }
    }

    val attach: @Composable () -> Unit = {
        if (attachments != null && platform != null) {
            MyButton({ platform.pickFiles(attachments.onAdd) }, icon = MyIcon.Paperclip, description = "Attach files", muted = true)
        }
    }

    val pickers: @Composable () -> Unit = {
        if (accounts.isNotEmpty() && accountLocked) {
            Label(conversation.account ?: "Default account", Modifier.padding(horizontal = Tokens.Space.S2).semantics { contentDescription = "Account: ${conversation.account ?: "default"}, fixed once the conversation has started" }, Tokens.Type.Ui, colors.textMuted, maxLines = 1)
        } else if (accounts.isNotEmpty()) {
            OptionMenu("Account", accounts, conversation.account ?: DefaultAccountId.takeIf { id -> accounts.any { it.id == id } }, onSetAccount, title = accountMenuTitle(conversation.agent), icon = MyIcon.Person)
        }

        if (modes.isNotEmpty()) {
            OptionMenu("Mode", modes, options.currentMode, onSetMode)
        }

        if (options.models.isNotEmpty()) {
            OptionMenu("Model", options.models, options.currentModel, onSetModel)
        } else if (options.currentModel != null) {
            Label(options.currentModel, Modifier.padding(horizontal = Tokens.Space.S2), Tokens.Type.Ui, colors.textMuted, maxLines = 1)
        }

        for (setting in settings) {
            OptionMenu(setting.name, setting.choices, setting.current, { value -> onSetSetting(setting.id, value) })
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
                    .border(Tokens.Size.Hairline, colors.border, shape)
                    .padding(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S1),
            ) {
                goal?.goal?.let { GoalBar(it, goal.onPause, goal.onResume, goal.onClear) }

                if (queue.isNotEmpty() || sending.isNotEmpty()) {
                    QueuedMessages(queue, sending, onSendQueuedNow, onEditQueued, onRemoveQueued)
                }

                AnnotationChips(annotations, onRemoveAnnotation)
                attachments?.let { AttachmentChips(it) }

                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    field(Modifier.weight(1f).padding(start = Tokens.Space.S2))
                    SendButtons(working, onStop, ::submit)
                }

                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    attach()
                    goal?.let { GoalButton(it) }
                    pickers()
                }
            }
        }

        return
    }

    Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        val measured = Modifier.widthIn(max = Tokens.Size.Measure).fillMaxWidth()

        goal?.goal?.let { held ->
            Box(measured) {
                GoalBar(held, goal.onPause, goal.onResume, goal.onClear)
            }
        }

        if (queue.isNotEmpty() || sending.isNotEmpty()) {
            Box(measured) {
                QueuedMessages(queue, sending, onSendQueuedNow, onEditQueued, onRemoveQueued)
            }
        }

        Column(
            Modifier
                .fillMaxWidth()
                .background(colors.bgContent)
                .drawBehind {
                    val line = Tokens.Size.Hairline.toPx()
                    drawRect(colors.border, Offset.Zero, androidx.compose.ui.geometry.Size(size.width, line))
                },
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Box(measured.padding(horizontal = Tokens.Space.S2)) {
                AnnotationChips(annotations, onRemoveAnnotation)
            }

            Box(measured.padding(horizontal = Tokens.Space.S2)) {
                attachments?.let { AttachmentChips(it) }
            }

            field(measured.padding(start = Tokens.Space.S4, top = Tokens.Space.S2, end = Tokens.Space.S4))

            Row(
                measured.padding(start = Tokens.Space.S2, top = Tokens.Space.S1, end = Tokens.Space.S2, bottom = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                attach()
                goal?.let { GoalButton(it) }
                Box(Modifier.weight(1f))

                if (working) {
                    Icon(MyIcon.Working, colors.statusWorking, modifier = Modifier.padding(horizontal = Tokens.Space.S1).semantics { contentDescription = "Working" })
                }

                pickers()
                SendButtons(working, onStop, ::submit)
            }
        }
    }
}

@Composable
private fun MessageField(text: TextFieldValue, onTextChange: (TextFieldValue) -> Unit, source: MutableInteractionSource, onSubmit: () -> Unit, onPaste: () -> Boolean, onKey: (Key) -> Boolean, modifier: Modifier = Modifier) {
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
                if (event.type == KeyEventType.KeyDown && onKey(event.key)) {
                    return@onPreviewKeyEvent true
                }

                if (event.type == KeyEventType.KeyDown && event.key == Key.V && (event.isCtrlPressed || event.isMetaPressed) && onPaste()) {
                    return@onPreviewKeyEvent true
                }

                val enter = event.key == Key.Enter && !event.isShiftPressed

                if (enter && event.type == KeyEventType.KeyDown) {
                    onSubmit()
                }

                enter
            },
        decorationBox = { inner ->
            Box(contentAlignment = Alignment.CenterStart) {
                if (text.text.isEmpty()) {
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
        muted = true,
    )
}

@Composable
fun OptionMenu(
    name: String,
    choices: List<Choice>,
    current: String?,
    onSelect: (String) -> Unit,
    title: String = name,
    icon: MyIcon? = null,
) {
    val buttonLabel = choices.firstOrNull { it.id == current }?.name ?: current ?: name
    var open by remember { mutableStateOf(false) }
    var search by remember(open) { mutableStateOf("") }
    val searchable = choices.size > SearchableChoices
    val shown = if (search.isBlank()) choices else choices.filter { it.name.contains(search, ignoreCase = true) || it.id.contains(search, ignoreCase = true) }

    Box {
        MyButton({ open = true }, text = buttonLabel, icon = icon, description = "$name: $buttonLabel", muted = true, trailingIcon = MyIcon.ChevronDown, selected = open, tooltip = name)

        if (open) {
            MenuPopup(onDismiss = { open = false }, width = if (choices.any { it.description != null }) Tokens.Size.Popover else Tokens.Size.Menu) {
                MenuLabel(title)

                if (searchable) {
                    TextInput(search, { search = it }, label = "Search ${name.lowercase()}", placeholder = "Select an option", showLabel = false, modifier = Modifier.padding(Tokens.Space.S1))
                }

                Column(Modifier.heightIn(max = Tokens.Size.MenuList).verticalScroll(rememberScrollState())) {
                    for (choice in shown) {
                        MenuItem(choice.name, onClick = { onSelect(choice.id); open = false }, checked = choice.id == current, hint = choice.description, meter = choice.usedPercent)
                    }
                }
            }
        }
    }
}

private const val SearchableChoices = 7

private const val FileSearchDelayMillis = 120L

private class Completion(val label: String, val description: String, val apply: () -> Unit)

@Composable
private fun CompletionPopup(title: String, completions: List<Completion>, highlighted: Int) {
    val colors = MyTheme.colors
    val gap = with(LocalDensity.current) { Tokens.Space.S1.roundToPx() }
    val selected = completions[highlighted]

    Popup(popupPositionProvider = remember(gap) { BelowAnchor(gap) }, properties = PopupProperties(focusable = false)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            val list = rememberLazyListState()

            LaunchedEffect(highlighted) {
                val target = highlighted + 1
                val layout = list.layoutInfo
                val first = layout.visibleItemsInfo.firstOrNull()?.index ?: return@LaunchedEffect
                val lastWhole = layout.visibleItemsInfo.lastOrNull { it.offset + it.size <= layout.viewportEndOffset }?.index ?: first

                when {
                    target <= first -> list.scrollToItem(if (highlighted == 0) 0 else target)
                    target > lastWhole -> list.scrollToItem(first + target - lastWhole)
                }
            }

            LazyColumn(
                Modifier
                    .width(Tokens.Size.Menu)
                    .heightIn(max = Tokens.Size.MenuList)
                    .background(colors.bgRaised, FloatShape)
                    .border(Tokens.Size.Hairline, colors.border, FloatShape)
                    .padding(Tokens.Space.S1)
                    .semantics { contentDescription = title },
                state = list,
            ) {
                item { MenuLabel(title) }

                itemsIndexed(completions) { index, completion ->
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = rowHeight())
                            .pressable(completion.apply, shape = ControlShape, selected = index == highlighted, description = completion.label)
                            .padding(horizontal = Tokens.Space.S2),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        Label(completion.label, style = Tokens.Type.Code, maxLines = 1)
                    }
                }
            }

            if (selected.description.isNotEmpty() && !LocalTouch.current) {
                Box(
                    Modifier
                        .width(Tokens.Size.Popover)
                        .background(colors.bgRaised, FloatShape)
                        .border(Tokens.Size.Hairline, colors.border, FloatShape)
                        .padding(Tokens.Space.S3),
                ) {
                    Label(selected.description, style = Tokens.Type.Ui)
                }
            }
        }
    }
}

@Composable
private fun QueuedMessages(
    queue: List<QueuedPrompt>,
    sending: List<String>,
    onSendNow: (String) -> Unit,
    onEdit: (String, String) -> Unit,
    onRemove: (String) -> Unit,
) {
    val colors = MyTheme.colors
    var expanded by remember { mutableStateOf(true) }
    var editing by remember { mutableStateOf<String?>(null) }
    var draft by remember { mutableStateOf("") }
    val touch = LocalTouch.current

    Column(Modifier.fillMaxWidth().semantics { contentDescription = "Queued messages" }) {
        Row(
            Modifier.fillMaxWidth().padding(start = Tokens.Space.S1, end = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MyButton(
                { expanded = !expanded },
                text = "${queue.size + sending.size} queued ${if (queue.size + sending.size == 1) "message" else "messages"}",
                description = if (expanded) "Collapse queued messages" else "Expand queued messages",
                size = ButtonSize.Sm,
                muted = true,
                trailingIcon = if (expanded) MyIcon.ChevronDown else MyIcon.ChevronRight,
            )
            Box(Modifier.weight(1f))

            if (queue.isNotEmpty()) {
                MyButton({ queue.forEach { onRemove(it.id) } }, text = "Clear all", size = ButtonSize.Sm, muted = true)
            }
        }

        if (!expanded) {
            Hairline()

            return@Column
        }

        for ((index, item) in queue.withIndex()) {
            val source = remember { MutableInteractionSource() }
            val hovered by source.collectIsHoveredAsState()
            var focused by remember { mutableStateOf(false) }
            val showActions = touch || hovered || focused || editing == item.id

            Row(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = rowHeight())
                    .hoverable(source)
                    .onFocusChanged { focused = it.hasFocus }
                    .padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (editing == item.id) {
                    TextInput(draft, { draft = it }, label = "Queued message", modifier = Modifier.weight(1f), showLabel = false)
                    MyButton({ onEdit(item.id, draft.trim()); editing = null }, text = "Save", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                    MyButton({ editing = null }, text = "Cancel", size = ButtonSize.Sm, muted = true)
                } else {
                    Box(Modifier.semantics { contentDescription = if (index == 0) "Next message" else "Queued" }) {
                        Dot(if (index == 0) colors.statusAttention else colors.textFaint, hollow = index != 0)
                    }

                    Label(item.text, Modifier.weight(1f), maxLines = 1)

                    Row(Modifier.alpha(if (showActions) 1f else 0f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                        MyButton({ onRemove(item.id) }, icon = MyIcon.Close, description = "Remove queued message", size = ButtonSize.Sm, muted = true)
                        MyButton({ draft = item.text; editing = item.id }, icon = MyIcon.Pencil, description = "Edit queued message", size = ButtonSize.Sm, muted = true)
                        MyButton({ onSendNow(item.id) }, text = "Send now", size = ButtonSize.Sm, muted = true)

                        if (index == 0) {
                            Label("Enter", style = Tokens.Type.Meta, color = colors.textFaint)
                        }
                    }
                }
            }

            Hairline()
        }

        for (text in sending) {
            Row(
                Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(MyIcon.Working, colors.textMuted, Tokens.Size.IconSm, Modifier.semantics { contentDescription = "Sending" })
                Label(text, Modifier.weight(1f), maxLines = 1)
                Label("Sending", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            Hairline()
        }
    }
}

@Composable
private fun AnnotationChips(annotations: List<Annotation>, onRemove: (Annotation) -> Unit) {
    if (annotations.isEmpty()) {
        return
    }

    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(vertical = Tokens.Space.S1),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (annotation in annotations) {
            val label = annotationLabel(annotation)

            Row(
                Modifier
                    .border(Tokens.Size.Hairline, MyTheme.colors.border, ControlShape)
                    .padding(start = Tokens.Space.S2)
                    .semantics { contentDescription = "Attached: $label" },
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(label, style = Tokens.Type.Meta, maxLines = 1)
                MyButton({ onRemove(annotation) }, icon = MyIcon.Close, description = "Remove attachment: $label", size = ButtonSize.Sm, muted = true)
            }
        }
    }
}
