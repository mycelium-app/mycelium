package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
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
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.runtime.produceState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
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

@Composable
fun Composer(
    conversation: Conversation,
    working: Boolean,
    queue: List<QueuedPrompt>,
    sending: List<String>,
    onSend: (String) -> Unit,
    onStop: () -> Unit,
    onSendQueuedNow: (String) -> Unit,
    onEditQueued: (QueuedPrompt) -> Unit,
    onRemoveQueued: (String) -> Unit,
    options: SessionOptions,
    commands: List<SlashCommand>,
    onSetMode: (String) -> Unit,
    onSetModel: (String) -> Unit,
    onSetSetting: (String, String) -> Unit,
    onSearchFiles: suspend (String) -> List<String>,
    modifier: Modifier = Modifier,
    accountPicker: AccountPickerControls? = null,
    onSideChat: (kind: String, text: String) -> Unit = { _, _ -> },
    annotations: List<Annotation> = emptyList(),
    onRemoveAnnotation: (Annotation) -> Unit = {},
    goal: GoalControls? = null,
    attachments: AttachmentControls? = null,
    dictation: Dictation? = null,
    initialText: TextFieldValue = TextFieldValue(""),
    onTextChange: (TextFieldValue) -> Unit = {},
    initialImages: Map<Char, Int> = emptyMap(),
    onImagesChange: (Map<Char, Int>) -> Unit = {},
    onSendAllQueuedNow: () -> Unit = {},
    onMoveQueued: (String, Int) -> Unit = { _, _ -> },
    queueEdit: QueueEdit? = null,
    onTeachMe: ((Boolean) -> Unit)? = null,
    assistantActions: AssistantActionControls? = null,
    usage: @Composable () -> Unit = {},
) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    var text by remember(conversation.id) { mutableStateOf(initialText) }
    var pastes by remember(conversation.id) { mutableStateOf(emptyMap<Char, String>()) }
    var images by remember(conversation.id) { mutableStateOf(initialImages) }
    var history by remember(conversation.id) { mutableStateOf(EditHistory<ComposerEdit>()) }
    val reportText by rememberUpdatedState(onTextChange)
    val reportImages by rememberUpdatedState(onImagesChange)
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
        snapshotFlow { images }.collect { reportImages(it) }
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
        attempt { files = onSearchFiles(wanted) }
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

        if (queueEdit != null) {
            queueEdit.onSave(trimmed)

            return
        }

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

    val fieldFocus = remember { FocusRequester() }
    val field: @Composable (Modifier) -> Unit = { fieldModifier ->
        Column(fieldModifier) {
            Box {
                MessageField(
                    modifier = Modifier.focusRequester(fieldFocus),
                    text = text,
                    visualTransformation = remember(display, colors.textMuted) { PasteMarkerTransformation(display, colors.textMuted) },
                    badges = display.badgeRanges().values.toList(),
                    previews = display.badgeRanges().mapNotNull { (marker, range) -> images[marker]?.let { id -> drafts.firstOrNull { it.id == id } }?.takeIf { it.file.mediaType.startsWith("image/") }?.let { range to it.file.bytes } }.toMap(),
                    onTextChange = ::changeText,
                    onToggleTeachMe = onTeachMe?.let { change -> { change(!conversation.teachMe) } },
                    onOpenActions = assistantActions?.let { { it.menu.open = true } },
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
                            key == Key.Escape && matches.isEmpty() && queueEdit != null -> { queueEdit.onCancel(); true }
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

    LaunchedEffect(queueEdit != null) {
        if (queueEdit != null) {
            fieldFocus.requestFocus()
        }
    }

    val editStrip: @Composable () -> Unit = {
        if (queueEdit != null) {
            QueueEditStrip(queueEdit, ::submit)
        }
    }
    val actionChips: @Composable () -> Unit = {
        if (assistantActions != null) {
            ActionChips(assistantActions)
        }
    }

    val controls = barControls(
        conversationKind(conversation, working),
        HarnessReport(voice = dictation != null, teachMe = onTeachMe != null, working = working, models = options.models.isNotEmpty(), modes = modes.isNotEmpty(), settings = settings.isNotEmpty()),
    ).filterNot { it == BarControl.Send && queueEdit != null }

    val control: @Composable (BarControl) -> Unit = { which ->
        when (which) {
            BarControl.Attach -> MyButton({ attachments?.let { platform?.pickFiles(it.onAdd) } }, icon = MyIcon.Paperclip, description = "Attach files", muted = true)
            BarControl.Actions -> assistantActions?.let { ActionsButton(it) }
            BarControl.Dictate -> MyButton(::toggleDictation, icon = MyIcon.Microphone, description = if (listening) "Stop dictating" else "Dictate", selected = listening, muted = !listening)
            BarControl.TeachMe -> MyButton(
                { onTeachMe?.invoke(!conversation.teachMe) },
                icon = MyIcon.GraduationCap,
                description = "Teach me",
                stateDescription = if (conversation.teachMe) "On" else "Off",
                tooltip = if (conversation.teachMe) "Teach me is on: hints and explanations, no code or file edits ($TeachMeShortcut)" else "Teach me: ask for hints and explanations instead of code ($TeachMeShortcut)",
                selected = conversation.teachMe,
                muted = !conversation.teachMe,
            )
            BarControl.Goal -> goal?.let { GoalButton(it) }
            BarControl.Account -> accountPicker?.let { AccountPicker(it) }
            BarControl.Model -> OptionMenu("Model", options.models, options.currentModel, onSetModel)
            BarControl.Mode -> OptionMenu("Mode", modes, options.currentMode, onSetMode)
            BarControl.Settings -> {
                for (setting in settings) {
                    OptionMenu(setting.name, setting.choices, setting.current, { value -> onSetSetting(setting.id, value) })
                }
            }
            BarControl.Usage -> usage()
            BarControl.Stop -> MyButton(onStop, icon = MyIcon.Stop, description = "Stop the agent", variant = ButtonVariant.Outline)
            BarControl.Send -> MyButton(::submit, icon = MyIcon.Send, description = if (working) "Queue message" else "Send", muted = true)
        }
    }

    if (LocalTouch.current) {
        val shape = RoundedCornerShape(Tokens.Radius.Card)
        var optionsOpen by remember(conversation.id) { mutableStateOf(false) }
        val accountName = accountPicker?.label
        val harnessChoices = listOfNotNull(
            (options.models.firstOrNull { it.id == options.currentModel }?.name ?: options.currentModel).takeIf { BarControl.Model in controls },
            (modes.firstOrNull { it.id == options.currentMode }?.name ?: options.currentMode).takeIf { BarControl.Mode in controls },
        )

        Column(modifier.fillMaxWidth().padding(start = Tokens.Space.S2, end = Tokens.Space.S2, bottom = Tokens.Space.S2)) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .shadow(Tokens.Space.S2, shape, ambientColor = colors.shadowFloat, spotColor = colors.shadowFloat)
                    .background(colors.bgRaised, shape)
                    .border(Tokens.Size.Hairline, colors.border, shape)
                    .padding(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S1),
            ) {
                goal?.goal?.let { GoalBar(it, goal) }

                if (queue.isNotEmpty() || sending.isNotEmpty()) {
                    QueuedMessages(queue, sending, onSendQueuedNow, onSendAllQueuedNow, onEditQueued, onMoveQueued, onRemoveQueued)
                }

                editStrip()
                actionChips()
                AnnotationChips(annotations, onRemoveAnnotation)
                unmarked?.let { AttachmentChips(it) }

                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    field(Modifier.weight(1f).padding(start = Tokens.Space.S2))
                    controls.inGroups(BarGroup.Turn).forEach { control(it) }
                }

                Row(Modifier.fillMaxWidth().horizontalScrollFaded(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    controls.inGroups(BarGroup.Tools).forEach { control(it) }

                    if (controls.inGroups(BarGroup.Pickers, BarGroup.Usage).isNotEmpty()) {
                        MyButton(
                            { optionsOpen = true },
                            text = harnessChoices.joinToString(" · ").ifEmpty { accountName ?: "Options" },
                            icon = MyIcon.Sliders,
                            description = "Conversation options: ${(listOfNotNull(accountName) + harnessChoices).joinToString(", ")}",
                            muted = true,
                            selected = optionsOpen,
                            maxWidth = Tokens.Size.AccountControl,
                        )
                    }
                }

                if (optionsOpen) {
                    BottomSheet({ optionsOpen = false }) {
                        MenuLabel("Conversation options")

                        Column(Modifier.padding(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                            controls.inGroups(BarGroup.Pickers, BarGroup.Usage).forEach { control(it) }
                        }
                    }
                }
            }
        }

        return
    }

    Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        val measured = Modifier.widthIn(max = readingMeasure()).fillMaxWidth()

        goal?.goal?.let { held ->
            Box(measured) {
                GoalBar(held, goal)
            }
        }

        if (queue.isNotEmpty() || sending.isNotEmpty()) {
            Box(measured) {
                QueuedMessages(queue, sending, onSendQueuedNow, onSendAllQueuedNow, onEditQueued, onMoveQueued, onRemoveQueued)
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
                editStrip()
            }

            Box(measured.padding(horizontal = Tokens.Space.S2)) {
                actionChips()
                AnnotationChips(annotations, onRemoveAnnotation)
            }

            Box(measured.padding(horizontal = Tokens.Space.S2)) {
                unmarked?.let { AttachmentChips(it) }
            }

            field(measured.padding(start = Tokens.Space.S4, top = Tokens.Space.S2, end = Tokens.Space.S4))

            ComposerBar(controls, measured.padding(start = Tokens.Space.S2, top = Tokens.Space.S1, end = Tokens.Space.S2, bottom = Tokens.Space.S2), control)
        }
    }
}

private const val TeachMeShortcut = "Ctrl or Cmd+Shift+T"

private fun dictationProblemText(problem: DictationProblem) = when (problem) {
    DictationProblem.Denied -> "Allow microphone access for Mycelium to dictate."
    DictationProblem.NoMicrophone -> "No microphone found. Connect one and try again."
    is DictationProblem.Failed -> problem.message
}

val LocalComposerPlaceholder = compositionLocalOf { "Ask, plan or build" }

@Composable
fun MessageField(text: TextFieldValue, onTextChange: (TextFieldValue) -> Unit, source: MutableInteractionSource, onSubmit: () -> Unit, onPaste: () -> Boolean, onKey: (Key) -> Boolean, modifier: Modifier = Modifier, onToggleTeachMe: (() -> Unit)? = null, onOpenActions: (() -> Unit)? = null, onUndo: (() -> Unit)? = null, onRedo: (() -> Unit)? = null, placeholder: String = LocalComposerPlaceholder.current, visualTransformation: VisualTransformation = VisualTransformation.None, alignment: Alignment = Alignment.CenterStart, badges: List<IntRange> = emptyList(), previews: Map<IntRange, ByteArray> = emptyMap()) {
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

                if (event.type == KeyEventType.KeyDown && event.key == Key.A && (event.isCtrlPressed || event.isMetaPressed) && event.isShiftPressed && onOpenActions != null) {
                    onOpenActions()

                    return@onPreviewKeyEvent true
                }

                if (event.type == KeyEventType.KeyDown && event.key == Key.T && (event.isCtrlPressed || event.isMetaPressed) && event.isShiftPressed && onToggleTeachMe != null) {
                    onToggleTeachMe()
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
                    val decoded by produceState<Pair<Boolean, ImageBitmap?>>(false to null, bytes) {
                        value = false to null
                        value = true to bytes?.let { withContext(Dispatchers.Default) { bitmapOf(it) } }
                    }
                    val anchor = shownRange?.let { range -> layout?.let { badgeLineRects(it, range).firstOrNull() } }

                    if (bytes != null && anchor != null) {
                        val gap = with(LocalDensity.current) { Tokens.Space.S2.roundToPx() }
                        val position = remember(anchor.left, gap) {
                            object : PopupPositionProvider {
                                override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize) =
                                    IntOffset(anchorBounds.left + anchor.left.toInt(), anchorBounds.top - gap - popupContentSize.height)
                            }
                        }
                        val image = decoded.second

                        Popup(popupPositionProvider = position, properties = PopupProperties(focusable = false)) {
                            Box(
                                Modifier
                                    .shadow(Tokens.Space.S2, FloatShape, ambientColor = colors.shadowFloat, spotColor = colors.shadowFloat)
                                    .background(colors.bgRaised, FloatShape)
                                    .border(Tokens.Size.Hairline, colors.border, FloatShape)
                                    .padding(Tokens.Space.S1),
                            ) {
                                if (image != null) {
                                    val natural = with(LocalDensity.current) { DpSize(image.width.toDp(), image.height.toDp()) }
                                    val fit = minOf(1f, Tokens.Size.ImagePreviewMaxWidth / natural.width, Tokens.Size.ImagePreviewMaxHeight / natural.height)

                                    Image(
                                        image,
                                        contentDescription = "Image preview",
                                        modifier = Modifier.size(natural * fit).clip(ControlShape),
                                        contentScale = ContentScale.Fit,
                                    )
                                } else {
                                    Column(
                                        Modifier.size(Tokens.Size.ImagePreviewLoadingWidth, Tokens.Size.ImagePreviewLoadingHeight),
                                        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.CenterVertically),
                                        horizontalAlignment = Alignment.CenterHorizontally,
                                    ) {
                                        if (decoded.first) {
                                            Label("The image could not be shown", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
                                        } else {
                                            Icon(MyIcon.Working, colors.statusWorking, Tokens.Size.IconSm)
                                            Label("Loading the image", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
                                        }
                                    }
                                }
                            }
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

class OptionFooter(val name: String, val hint: String, val icon: MyIcon, val onClick: () -> Unit)

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
    label: String? = null,
    footer: OptionFooter? = null,
    buttonText: String? = null,
    tooltip: String? = null,
    maxWidth: Dp? = null,
) {
    val buttonLabel = buttonText ?: choices.firstOrNull { it.id == current }?.name ?: label ?: name
    var open by remember { mutableStateOf(false) }
    var search by remember(open) { mutableStateOf("") }
    val searchable = choices.size > SearchableChoices
    val shown = if (search.isBlank()) choices else choices.filter { it.name.contains(search, ignoreCase = true) || it.id.contains(search, ignoreCase = true) }

    Box(if (fill) Modifier.fillMaxWidth() else Modifier) {
        MyButton({ open = true }, text = buttonLabel, icon = icon, description = "$name: $buttonLabel", size = size, muted = true, trailingIcon = MyIcon.ChevronDown, selected = open, tooltip = tooltip ?: name, fill = fill, maxWidth = maxWidth)

        if (open) {
            MenuPopup(onDismiss = { open = false }, width = if (choices.any { it.description != null }) Tokens.Size.Popover else Tokens.Size.Menu) {
                MenuLabel(title)

                if (searchable) {
                    TextInput(search, { search = it }, label = "Search ${name.lowercase()}", placeholder = "Select an option", showLabel = false, modifier = Modifier.padding(Tokens.Space.S1))
                }

                Column(Modifier.heightIn(max = Tokens.Size.MenuList).verticalScrollWithBar()) {
                    for ((index, choice) in shown.withIndex()) {
                        if (choice.group != null && choice.group != shown.getOrNull(index - 1)?.group) {
                            MenuLabel(choice.group)
                        }

                        MenuItem(choice.name, onClick = { onSelect(choice.id); open = false }, checked = choice.id == current, hint = choice.description, meter = choice.usedPercent)
                    }
                }

                if (footer != null) {
                    Hairline(Modifier.padding(vertical = Tokens.Space.S1))
                    MenuItem(footer.name, onClick = { footer.onClick(); open = false }, leading = footer.icon, hint = footer.hint)
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
