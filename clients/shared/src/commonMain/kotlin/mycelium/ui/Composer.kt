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
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.foundation.Image
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.layout.size
import org.jetbrains.compose.resources.decodeToImageBitmap
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.rememberUpdatedState
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
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import mycelium.Dictation
import mycelium.DictationProblem
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
import mycelium.state.expandedPastes
import mycelium.state.freePasteMarker
import mycelium.state.isPasteMarker
import mycelium.state.BadgeGap
import mycelium.state.MarkedText
import mycelium.state.EditHistory
import mycelium.state.isImage
import mycelium.state.orphanedImageIds
import mycelium.state.withImageMarkers
import mycelium.state.withoutImageMarkers
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.TextLayoutResult
import mycelium.state.pastedLabel
import mycelium.state.PasteDisplay
import mycelium.state.pastedSummary
import mycelium.state.pasteInsertion
import mycelium.state.serialisedPastes
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

data class ContinueControls(val choices: List<Choice>, val onPick: (String) -> Unit)

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
    continueOn: ContinueControls? = null,
    onSetAccount: (String) -> Unit = {},
    onSideChat: (kind: String, text: String) -> Unit = { _, _ -> },
    annotations: List<Annotation> = emptyList(),
    onRemoveAnnotation: (Annotation) -> Unit = {},
    goal: GoalControls? = null,
    attachments: AttachmentControls? = null,
    modelPicker: ModelPickerControls? = null,
    dictation: Dictation? = null,
    initialText: TextFieldValue = TextFieldValue(""),
    onTextChange: (TextFieldValue) -> Unit = {},
    onSendAllQueuedNow: () -> Unit = {},
    usage: @Composable () -> Unit = {},
) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    var text by remember(conversation.id) { mutableStateOf(initialText) }
    var pastes by remember(conversation.id) { mutableStateOf(emptyMap<Char, String>()) }
    var images by remember(conversation.id) { mutableStateOf(emptyMap<Char, Int>()) }
    var history by remember(conversation.id) { mutableStateOf(EditHistory<ComposerEdit>()) }
    val reportText by rememberUpdatedState(onTextChange)
    val drafts = attachments?.drafts.orEmpty()
    val removeDraft by rememberUpdatedState(attachments?.onRemove)
    val currentDrafts by rememberUpdatedState(drafts)

    LaunchedEffect(drafts) {
        val live = drafts.map { it.id }.toSet()
        val cleaned = withoutImageMarkers(MarkedText(text.text, text.selection.end, images), images.values.toSet() - live)
        val fresh = drafts.filter { isImage(it.file.mediaType) && it.id !in cleaned.images.values }.map { it.id }
        val marked = withImageMarkers(cleaned, fresh, pastes.keys)

        if (marked.text != text.text) {
            text = TextFieldValue(marked.text, TextRange(marked.cursor))
        }

        images = marked.images
    }

    LaunchedEffect(conversation.id) {
        snapshotFlow { text.text to images }.collect { (value, held) ->
            val orphaned = orphanedImageIds(value, held)

            if (orphaned.isNotEmpty()) {
                images = held.filterValues { it !in orphaned }
                currentDrafts.filter { it.id in orphaned }.forEach { removeDraft?.invoke(it) }
            }
        }
    }

    LaunchedEffect(conversation.id) {
        snapshotFlow { text to pastes }.collect { (value, held) ->
            val expanded = expandedPastes(value.text, held)

            reportText(if (expanded == value.text) value else TextFieldValue(expanded, TextRange(expanded.length)))
        }
    }

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
        val trimmed = serialisedPastes(text.text, pastes).trim()

        if (trimmed.isNotEmpty() || annotations.isNotEmpty() || attachments?.drafts?.isNotEmpty() == true) {
            val side = sideChatOf(trimmed)

            if (side != null) {
                onSideChat(side.first, side.second)
            } else {
                onSend(trimmed)
            }

            sent = text.text
            text = TextFieldValue("")
            pastes = emptyMap()
            images = emptyMap()
            history = EditHistory()
        } else if (queue.isNotEmpty()) {
            onSendQueuedNow(queue.minBy { it.position }.id)
        }
    }

    fun restore(edit: ComposerEdit) {
        val live = currentDrafts.map { it.id }.toSet()
        val cleaned = withoutImageMarkers(MarkedText(edit.text.text, edit.text.selection.end, edit.images), edit.images.values.toSet() - live)

        text = TextFieldValue(cleaned.text, TextRange(cleaned.cursor))
        pastes = edit.pastes
        images = cleaned.images
    }

    val display = remember(text.text, pastes, images, drafts) {
        PasteDisplay(text.text, pastes, images.mapNotNull { (marker, id) -> drafts.firstOrNull { it.id == id }?.let { marker to it.file.name } }.toMap())
    }
    val unmarked = attachments?.let { held -> AttachmentControls(held.drafts.filter { it.id !in images.values }, held.problem, held.onAdd, held.onRemove) }

    var listening by remember(conversation.id) { mutableStateOf(false) }
    var dictationProblem by remember(conversation.id) { mutableStateOf<DictationProblem?>(null) }
    val dictationOpen = remember(conversation.id, dictation) { booleanArrayOf(true) }

    fun changeText(changed: TextFieldValue) {
        val before = ComposerEdit(text, pastes, images)
        val stale = sent?.takeIf { it.isNotEmpty() && changed.text.startsWith(it) }
        sent = null

        val next = if (stale == null) changed else TextFieldValue(changed.text.removePrefix(stale), TextRange(changed.selection.end - stale.length))
        val range = pasteInsertion(text.text, next.text)
        val marker = freePasteMarker(pastes.keys + text.text.filter(::isPasteMarker).toSet())

        if (range == null || marker == null) {
            text = next
        } else {
            pastes = pastes + (marker to next.text.substring(range.first, range.last + 1).filterNot(::isPasteMarker))
            text = TextFieldValue(next.text.replaceRange(range, marker.toString()), TextRange(range.first + 1))
        }

        if (text.text != before.text.text) {
            val typedOne = text.text.length == before.text.text.length + 1 && pastes == before.pastes
            val typedSpace = typedOne && text.text.getOrNull(text.selection.end - 1)?.isWhitespace() == true
            history = history.recorded(before, typedOne, typedSpace)
        }
    }

    fun toggleDictation() {
        dictationProblem = null

        if (listening) {
            dictation?.stop()
            return
        }

        listening = true
        dictation?.start(
            onHeard = { heard ->
                if (!dictationOpen[0] || heard.isBlank()) {
                    return@start
                }

                val cursor = text.selection.end
                val separator = if (cursor > 0 && !text.text[cursor - 1].isWhitespace()) " " else ""
                val inserted = separator + heard
                val updated = text.text.substring(0, cursor) + inserted + text.text.substring(text.selection.end)

                changeText(TextFieldValue(updated, TextRange(cursor + inserted.length)))
            },
            onStopped = { problem ->
                listening = false
                dictationProblem = problem
            },
        )
    }

    DisposableEffect(conversation.id, dictation) {
        onDispose {
            dictationOpen[0] = false

            if (listening) {
                dictation?.stop()
            }
        }
    }

    val field: @Composable (Modifier) -> Unit = { fieldModifier ->
        Column(fieldModifier) {
            Box {
                MessageField(
                    text = text,
                    visualTransformation = remember(display, colors.textMuted) { PasteMarkerTransformation(display, colors.textMuted) },
                    badges = display.badgeRanges().values.toList(),
                    previews = display.badgeRanges().mapNotNull { (marker, range) -> images[marker]?.let { id -> drafts.firstOrNull { it.id == id } }?.takeIf { it.file.mediaType.startsWith("image/") }?.let { range to it.file.bytes } }.toMap(),
                    onTextChange = ::changeText,
                    onUndo = { history.undone(ComposerEdit(text, pastes, images))?.let { (rest, edit) -> history = rest; restore(edit) } },
                    onRedo = { history.redone(ComposerEdit(text, pastes, images))?.let { (rest, edit) -> history = rest; restore(edit) } },
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

            dictationProblem?.let { problem ->
                Row(Modifier.semantics { liveRegion = LiveRegionMode.Assertive }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    Icon(MyIcon.Failed, colors.statusFailed, Tokens.Size.IconSm)
                    Label(dictationProblemText(problem), style = Tokens.Type.Meta, color = colors.statusFailed)
                }
            }
        }
    }

    val attach: @Composable () -> Unit = {
        if (attachments != null && platform != null) {
            MyButton({ platform.pickFiles(attachments.onAdd) }, icon = MyIcon.Paperclip, description = "Attach files", muted = true)
        }
    }

    val dictate: @Composable () -> Unit = {
        if (dictation != null) {
            MyButton(::toggleDictation, icon = MyIcon.Microphone, description = if (listening) "Stop dictating" else "Dictate", selected = listening, muted = !listening)
        }
    }

    val pickers: @Composable () -> Unit = {
        if (accountLocked && continueOn != null && continueOn.choices.isNotEmpty()) {
            OptionMenu("Continue on…", continueOn.choices, conversation.account ?: "Default account", continueOn.onPick, title = "Continue on", icon = MyIcon.Person)
        } else if (accounts.isNotEmpty() && accountLocked) {
            Label(conversation.account ?: "Default account", Modifier.padding(horizontal = Tokens.Space.S2).semantics { contentDescription = "Account: ${conversation.account ?: "default"}, fixed once the conversation has started" }, Tokens.Type.Ui, colors.textMuted, maxLines = 1)
        } else if (accounts.isNotEmpty()) {
            OptionMenu("Account", accounts, conversation.account ?: DefaultAccountId.takeIf { id -> accounts.any { it.id == id } }, onSetAccount, title = accountMenuTitle(conversation.agent), icon = MyIcon.Person)
        }

        if (modes.isNotEmpty()) {
            OptionMenu("Mode", modes, options.currentMode, onSetMode)
        }

        if (modelPicker != null) {
            ModelPicker(modelPicker)
        } else if (options.models.isNotEmpty()) {
            OptionMenu("Model", options.models, options.currentModel, onSetModel)
        } else if (options.currentModel != null) {
            Label(options.currentModel, Modifier.padding(horizontal = Tokens.Space.S2), Tokens.Type.Ui, colors.textMuted, maxLines = 1)
        }

        for (setting in settings) {
            OptionMenu(setting.name, setting.choices, setting.current, { value -> onSetSetting(setting.id, value) })
        }

        if (modelPicker != null && continueOn != null && continueOn.choices.isNotEmpty()) {
            OptionMenu("Continue on…", continueOn.choices, null, continueOn.onPick, title = "Continue on", icon = MyIcon.Person)
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
                    QueuedMessages(queue, sending, onSendQueuedNow, onSendAllQueuedNow, onEditQueued, onRemoveQueued)
                }

                AnnotationChips(annotations, onRemoveAnnotation)
                unmarked?.let { AttachmentChips(it) }

                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    field(Modifier.weight(1f).padding(start = Tokens.Space.S2))
                    SendButtons(working, onStop, ::submit)
                }

                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    Row(Modifier.weight(1f).horizontalScrollFaded(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                        attach()
                        dictate()
                        goal?.let { GoalButton(it) }
                        pickers()
                    }

                    usage()
                }
            }
        }

        return
    }

    Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        val measured = Modifier.widthIn(max = readingMeasure()).fillMaxWidth()

        goal?.goal?.let { held ->
            Box(measured) {
                GoalBar(held, goal.onPause, goal.onResume, goal.onClear)
            }
        }

        if (queue.isNotEmpty() || sending.isNotEmpty()) {
            Box(measured) {
                QueuedMessages(queue, sending, onSendQueuedNow, onSendAllQueuedNow, onEditQueued, onRemoveQueued)
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
                unmarked?.let { AttachmentChips(it) }
            }

            field(measured.padding(start = Tokens.Space.S4, top = Tokens.Space.S2, end = Tokens.Space.S4))

            Row(
                measured.padding(start = Tokens.Space.S2, top = Tokens.Space.S1, end = Tokens.Space.S2, bottom = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                attach()
                dictate()
                goal?.let { GoalButton(it) }
                Row(
                    Modifier.weight(1f).horizontalScrollFaded(),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1, Alignment.End),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (working) {
                        Icon(MyIcon.Working, colors.statusWorking, modifier = Modifier.padding(horizontal = Tokens.Space.S1).semantics { contentDescription = "Working" })
                    }

                    pickers()
                }

                usage()
                SendButtons(working, onStop, ::submit)
            }
        }
    }
}

private fun dictationProblemText(problem: DictationProblem) = when (problem) {
    DictationProblem.Denied -> "Allow microphone access for Mycelium to dictate."
    DictationProblem.NoMicrophone -> "No microphone found. Connect one and try again."
    is DictationProblem.Failed -> problem.message
}

@Composable
fun MessageField(text: TextFieldValue, onTextChange: (TextFieldValue) -> Unit, source: MutableInteractionSource, onSubmit: () -> Unit, onPaste: () -> Boolean, onKey: (Key) -> Boolean, modifier: Modifier = Modifier, onUndo: (() -> Unit)? = null, onRedo: (() -> Unit)? = null, placeholder: String = "Ask, plan or build", visualTransformation: VisualTransformation = VisualTransformation.None, alignment: Alignment = Alignment.CenterStart, badges: List<IntRange> = emptyList(), previews: Map<IntRange, ByteArray> = emptyMap()) {
    val colors = MyTheme.colors
    var layout by remember { mutableStateOf<TextLayoutResult?>(null) }

    BasicTextField(
        value = text,
        onValueChange = onTextChange,
        textStyle = Tokens.Type.Prose.themed().copy(color = colors.text),
        cursorBrush = SolidColor(colors.text),
        interactionSource = source,
        visualTransformation = visualTransformation,
        onTextLayout = { layout = it },
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

                if (event.type == KeyEventType.KeyDown && event.key == Key.Backspace && (event.isCtrlPressed || event.isAltPressed) && text.selection.collapsed) {
                    val end = text.selection.end
                    var start = end

                    while (start > 0 && text.text[start - 1].isWhitespace()) start--

                    if (start > 0 && isPasteMarker(text.text[start - 1])) {
                        start--
                    } else {
                        while (start > 0 && !text.text[start - 1].isWhitespace() && !isPasteMarker(text.text[start - 1])) start--
                    }

                    onTextChange(TextFieldValue(text.text.removeRange(start, end), TextRange(start)))
                    return@onPreviewKeyEvent true
                }

                val shortcut = event.isCtrlPressed || event.isMetaPressed
                val redo = shortcut && ((event.key == Key.Z && event.isShiftPressed) || event.key == Key.Y)
                val undo = shortcut && event.key == Key.Z && !event.isShiftPressed

                if (onUndo != null && onRedo != null && (undo || redo)) {
                    if (event.type == KeyEventType.KeyDown && undo) {
                        onUndo()
                    }

                    if (event.type == KeyEventType.KeyDown && redo) {
                        onRedo()
                    }

                    return@onPreviewKeyEvent true
                }

                val enter = event.key == Key.Enter && !event.isShiftPressed

                if (enter && event.type == KeyEventType.KeyDown) {
                    onSubmit()
                }

                enter
            },
        decorationBox = { inner ->
            Box(contentAlignment = alignment) {
                if (text.text.isEmpty()) {
                    Label(placeholder, style = Tokens.Type.Prose, color = colors.textFaint)
                }

                var previewed by remember { mutableStateOf<IntRange?>(null) }
                val previewAt = { position: Offset -> layout?.let { measured -> previews.keys.firstOrNull { range -> badgeLineRects(measured, range).any { it.contains(position) } } } }

                Box(
                    Modifier.pointerInput(previews) {
                        awaitPointerEventScope {
                            while (true) {
                                val event = awaitPointerEvent()
                                val position = event.changes.firstOrNull()?.position ?: continue

                                previewed = when (event.type) {
                                    PointerEventType.Exit -> null
                                    PointerEventType.Press, PointerEventType.Move, PointerEventType.Enter -> previewAt(position)
                                    else -> previewed
                                }
                            }
                        }
                    }.drawBehind {
                        val measured = layout ?: return@drawBehind
                        val radius = CornerRadius(Tokens.Radius.Control.toPx())
                        val sideGap = Tokens.Space.S0_5.toPx()
                        val line = Tokens.Size.Hairline.toPx()

                        for (range in badges) {
                            for (rect in badgeLineRects(measured, range)) {
                                val box = Rect(rect.left - sideGap, rect.top + sideGap, rect.right + sideGap, rect.bottom - sideGap)

                                drawRoundRect(colors.bgRaised, box.topLeft, box.size, radius)
                                drawRoundRect(colors.border, box.topLeft, box.size, radius, Stroke(line))
                            }
                        }
                    },
                ) {
                    inner()

                    val shownRange = previewed
                    val bytes = shownRange?.let(previews::get)
                    val image = remember(bytes) { bytes?.let { runCatching { it.decodeToImageBitmap() }.getOrNull() } }
                    val anchor = shownRange?.let { range -> layout?.let { badgeLineRects(it, range).firstOrNull() } }

                    if (image != null && anchor != null) {
                        val lift = with(LocalDensity.current) { (Tokens.Size.ImagePreview + Tokens.Space.S2).toPx() }

                        Popup(offset = IntOffset(anchor.left.toInt(), (anchor.top - lift).toInt()), properties = PopupProperties(focusable = false)) {
                            Image(image, contentDescription = "Image preview", modifier = Modifier.size(Tokens.Size.ImagePreview).clip(ControlShape).border(Tokens.Size.Hairline, colors.border, ControlShape), contentScale = ContentScale.Crop)
                        }
                    }
                }
            }
        },
    )
}

private fun badgeLineRects(layout: TextLayoutResult, range: IntRange): List<Rect> {
    val length = layout.layoutInput.text.length
    val offsets = range.filter { it < length }

    return offsets.groupBy { layout.getLineForOffset(it) }.values.map { line ->
        line.map { layout.getBoundingBox(it) }.reduce { union, box -> Rect(minOf(union.left, box.left), minOf(union.top, box.top), maxOf(union.right, box.right), maxOf(union.bottom, box.bottom)) }
    }
}

private data class ComposerEdit(val text: TextFieldValue, val pastes: Map<Char, String>, val images: Map<Char, Int>)

private class PasteMarkerTransformation(private val display: PasteDisplay, private val labelColour: Color) : VisualTransformation {
    override fun filter(text: AnnotatedString): TransformedText {
        val shown = buildAnnotatedString {
            for (char in text.text) {
                val label = display.labelOf(char)

                if (label == null) {
                    append(char)
                } else {
                    append(BadgeGap)

                    withStyle(SpanStyle(fontSize = Tokens.Type.Meta.fontSize, color = labelColour)) {
                        append(label)
                    }

                    append(BadgeGap)
                }
            }
        }

        val mapping = object : OffsetMapping {
            override fun originalToTransformed(offset: Int): Int = display.toShown(offset)

            override fun transformedToOriginal(offset: Int): Int = display.toOriginal(offset)
        }

        return TransformedText(shown, mapping)
    }
}

@Composable
internal fun PastedBadge(lines: Int, expanded: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .heightIn(min = Tokens.Size.ControlSm)
            .pressable(onClick, role = Role.Button, description = pastedLabel(lines), shape = ControlShape, stateDescription = if (expanded) "Expanded" else "Collapsed")
            .padding(horizontal = Tokens.Space.S2),
        contentAlignment = Alignment.CenterStart,
    ) {
        Label(pastedLabel(lines), style = Tokens.Type.Meta, maxLines = 1)
    }
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
    size: ButtonSize = ButtonSize.Md,
    fill: Boolean = false,
) {
    val buttonLabel = choices.firstOrNull { it.id == current }?.name ?: current ?: name
    var open by remember { mutableStateOf(false) }
    var search by remember(open) { mutableStateOf("") }
    val searchable = choices.size > SearchableChoices
    val shown = if (search.isBlank()) choices else choices.filter { it.name.contains(search, ignoreCase = true) || it.id.contains(search, ignoreCase = true) }

    Box(if (fill) Modifier.fillMaxWidth() else Modifier) {
        MyButton({ open = true }, text = buttonLabel, icon = icon, description = "$name: $buttonLabel", size = size, muted = true, trailingIcon = MyIcon.ChevronDown, selected = open, tooltip = name, fill = fill)

        if (open) {
            MenuPopup(onDismiss = { open = false }, width = if (choices.any { it.description != null }) Tokens.Size.Popover else Tokens.Size.Menu) {
                MenuLabel(title)

                if (searchable) {
                    TextInput(search, { search = it }, label = "Search ${name.lowercase()}", placeholder = "Select an option", showLabel = false, modifier = Modifier.padding(Tokens.Space.S1))
                }

                Column(Modifier.heightIn(max = Tokens.Size.MenuList).verticalScrollWithBar()) {
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
                    .scrollbar(list)
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
    onSendAllNow: () -> Unit,
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
                MyButton(onSendAllNow, text = "Send all now", size = ButtonSize.Sm, muted = true)
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

                    Label(pastedSummary(item.text).replace('\n', ' ').trim(), Modifier.weight(1f), maxLines = 1)

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
