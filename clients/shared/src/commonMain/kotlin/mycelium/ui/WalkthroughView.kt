package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.unit.dp
import mycelium.api.Walkthrough
import mycelium.api.WalkthroughFile
import mycelium.api.WalkthroughNote
import mycelium.api.WalkthroughNoteRequest
import mycelium.api.WalkthroughStep
import mycelium.state.Annotation
import mycelium.state.FileDiff
import mycelium.state.Store
import mycelium.state.ToolStatus
import mycelium.state.WalkthroughScope
import mycelium.state.nextUnreviewed
import mycelium.state.reviewProgress
import mycelium.state.walkthroughDiff
import mycelium.state.walkthroughLines
import mycelium.state.walkthroughNoteLabel
import mycelium.state.walkthroughSections

private const val NotCoveredTitle = "Not covered yet"
private val StackedBelow = 640.dp

private fun sectionIcon(name: String?): MyIcon = when (name) {
    "logic" -> MyIcon.Code
    "tests" -> MyIcon.Flask
    "docs" -> MyIcon.File
    "config" -> MyIcon.Sliders
    "ui" -> MyIcon.Desktop
    "data" -> MyIcon.Archive
    else -> MyIcon.Folder
}

@Composable
fun WalkthroughView(store: Store, conversationId: String, walkthrough: Walkthrough, modifier: Modifier = Modifier) {
    var selected by remember(walkthrough.id) { mutableStateOf(0) }
    val scopes by store.walkthroughScopes.collectAsState()
    val allNotes by store.walkthroughNotes.collectAsState()
    val notes = allNotes[conversationId].orEmpty()
    var uncovered by remember(walkthrough.id) { mutableStateOf(emptyList<WalkthroughFile>()) }
    val steps = walkthrough.steps +
        listOfNotNull(WalkthroughStep(NotCoveredTitle, section = "Not covered", explanation = "These changes are not in any step above.", files = uncovered).takeIf { uncovered.isNotEmpty() }) +
        listOfNotNull(WalkthroughStep(CheckStepTitle, section = "Check").takeIf { walkthrough.checks.isNotEmpty() })
    val last = steps.lastIndex
    val index = selected.coerceIn(0, maxOf(last, 0))
    val reviewable = walkthrough.steps.size
    val reviewed = walkthrough.reviewed.toSet()

    LaunchedEffect(walkthrough.id) {
        store.loadWalkthroughNotes(conversationId, walkthrough.id)
    }

    LaunchedEffect(walkthrough.id) {
        uncovered = emptyList()
        attempt { uncovered = store.api.walkthroughUncovered(walkthrough.id) }
    }

    fun move(to: Int) {
        selected = to.coerceIn(0, maxOf(last, 0))
    }

    fun next() {
        move(index + 1)
    }

    fun toggleReviewed(step: Int) {
        store.setWalkthroughStepReviewed(conversationId, step, step !in reviewed)
    }

    fun approve(step: Int) {
        if (step in reviewed) {
            toggleReviewed(step)
            return
        }

        toggleReviewed(step)
        nextUnreviewed(step, reviewed + step, reviewable)?.let(::move)
    }

    val stepList: @Composable () -> Unit = {
        for (section in walkthroughSections(steps)) {
            if (section.name != null) {
                SectionHeader(section.name, sectionIcon(section.icon), section.steps.all { it.index < reviewable && it.index in reviewed })
            }

            for ((position, step) in section.steps) {
                val toggle = if (position < reviewable) { { toggleReviewed(position) } } else null
                StepRow(position, step, position == index, position in reviewed, onClick = { selected = position }, onToggleReviewed = toggle)
            }
        }
    }
    val detail: @Composable () -> Unit = {
        if (steps.isNotEmpty() && walkthrough.checks.isNotEmpty() && index == last) {
            WalkthroughCheckStep(store, walkthrough, onBackToStep = ::move)
        } else if (steps.isNotEmpty()) {
            walkthrough.note?.let { Label(it, color = MyTheme.colors.textMuted) }
            StepDetail(
                step = steps[index],
                index = index,
                walkthrough = walkthrough,
                notes = notes.filter { it.step == index },
                annotatable = index < walkthrough.steps.size,
                approved = index in reviewed,
                onApprove = { approve(index) }.takeIf { index < reviewable },
                onAnnotate = { file, diff, annotation ->
                    val lines = walkthroughLines(diff, annotation.quote)
                    store.addWalkthroughNote(conversationId, walkthrough.id, WalkthroughNoteRequest(index, file.path, lines?.first, lines?.second, annotation.quote, annotation.comment))
                },
                onNote = { text -> store.addWalkthroughNote(conversationId, walkthrough.id, WalkthroughNoteRequest(index, comment = text)) },
                onRemoveNote = { note -> store.removeWalkthroughNote(conversationId, walkthrough.id, note) },
            )
        }
    }
    val header: @Composable () -> Unit = {
        WalkthroughHeader(
            scope = scopes[conversationId] ?: WalkthroughScope.Branch,
            position = reviewProgress(reviewed.count { it < reviewable }, reviewable),
            onPrevious = index.takeIf { it > 0 }?.let { { move(index - 1) } },
            onNext = index.takeIf { it < last }?.let { { next() } },
            onScope = { store.chooseWalkthroughScope(conversationId, it) },
            onIterate = { comment -> store.iterateWalkthrough(conversationId, comment) },
            onDismiss = { store.setWalkthroughDismissed(conversationId, true) }.takeIf { LocalTouch.current },
        )
    }
    val stepKeys = Modifier.onKeyEvent { event ->
        val forward = event.key == Key.DirectionDown || event.key == Key.J
        val backward = event.key == Key.DirectionUp || event.key == Key.K

        if (event.type != KeyEventType.KeyDown || !(forward || backward)) {
            return@onKeyEvent false
        }

        if (forward) {
            next()
        } else {
            move(index - 1)
        }

        true
    }

    BoxWithConstraints(modifier.fillMaxSize()) {
        if (LocalTouch.current || maxWidth < StackedBelow) {
            Column(Modifier.fillMaxSize().verticalScrollWithBar().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                header()
                Column(stepKeys.semantics { contentDescription = "Walkthrough steps" }) { stepList() }
                Hairline()
                detail()
            }
        } else {
            Column(Modifier.fillMaxSize()) {
                Box(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1_5)) { header() }
                Hairline()

                Row(Modifier.weight(1f).fillMaxWidth()) {
                    Column(stepKeys.width(Tokens.Size.Sidebar).fillMaxHeight().verticalScrollWithBar().padding(Tokens.Space.S2).semantics { contentDescription = "Walkthrough steps" }) {
                        stepList()
                    }

                    VerticalHairline()

                    Column(Modifier.weight(1f).fillMaxHeight().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                        detail()
                    }
                }
            }
        }
    }
}

@Composable
private fun WalkthroughHeader(scope: WalkthroughScope, position: String, onPrevious: (() -> Unit)?, onNext: (() -> Unit)?, onScope: (WalkthroughScope) -> Unit, onIterate: (String) -> Unit, onDismiss: (() -> Unit)?) {
    var open by remember { mutableStateOf(false) }
    var iterating by remember { mutableStateOf(false) }
    var comment by remember { mutableStateOf("") }

    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        Box {
            MyButton({ open = true }, text = scope.label, icon = MyIcon.Branch, trailingIcon = MyIcon.ChevronDown, description = "Walkthrough scope: ${scope.label}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)

            if (open) {
                MenuPopup(onDismiss = { open = false }) {
                    MenuLabel("Walkthrough of")

                    for (option in WalkthroughScope.entries) {
                        MenuItem(option.label, { onScope(option); open = false }, checked = option == scope)
                    }
                }
            }
        }

        Box(Modifier.weight(1f))

        Label(position, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)

        MyButton({ onPrevious?.invoke() }, icon = MyIcon.Back, description = "Previous step", muted = onPrevious == null, size = ButtonSize.Sm)
        MyButton({ onNext?.invoke() }, icon = MyIcon.Move, description = "Next step", muted = onNext == null, size = ButtonSize.Sm)
        Box {
            MyButton({ iterating = true }, text = "Iterate", icon = MyIcon.Refresh, variant = ButtonVariant.Outline, size = ButtonSize.Sm, selected = iterating)

            if (iterating) {
                MenuPopup(onDismiss = { iterating = false }, width = Tokens.Size.Popover) {
                    fun iterate() {
                        onIterate(comment.trim())
                        comment = ""
                        iterating = false
                    }

                    Column(Modifier.padding(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        TextInput(comment, { comment = it }, label = "Comment for the agent (optional)", placeholder = "What should change", onSubmit = ::iterate)
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                            MyButton(::iterate, text = "Iterate", description = "Iterate with this comment", variant = ButtonVariant.Primary, size = ButtonSize.Sm)
                        }
                    }
                }
            }
        }

        if (onDismiss != null) {
            MyButton({ onDismiss() }, text = "Dismiss", icon = MyIcon.Close, description = "Dismiss walkthrough", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }
    }
}

@Composable
private fun SectionHeader(name: String, icon: MyIcon, reviewed: Boolean) {
    val colors = MyTheme.colors

    Row(
        Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5).semantics(mergeDescendants = true) {
            heading()

            if (reviewed) {
                contentDescription = "$name, reviewed"
            }
        },
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(if (reviewed) MyIcon.CircleCheck else icon, colors.textMuted, Tokens.Size.IconSm)
        Label(name.uppercase(), style = Tokens.Type.Meta, color = colors.textMuted)
    }
}

@Composable
private fun StepRow(position: Int, step: WalkthroughStep, on: Boolean, reviewed: Boolean, onClick: () -> Unit, onToggleReviewed: (() -> Unit)?) {
    val colors = MyTheme.colors
    val notes = listOfNotNull("key change".takeIf { step.key })

    Row(
        Modifier.fillMaxWidth().heightIn(min = rowHeight()).background(if (on) colors.bgSelected else Color.Transparent, ControlShape).padding(end = Tokens.Space.S1),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            Modifier
                .weight(1f)
                .heightIn(min = rowHeight())
                .pressable(onClick, role = Role.Tab, description = (listOf("Step ${position + 1}: ${step.title}") + notes).joinToString(", "), shape = ControlShape, selected = on)
                .semantics { selected = on }
                .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            NumberChip(position, on)

            Label(step.title, Modifier.weight(1f), if (step.key || on) Tokens.Type.UiStrong else Tokens.Type.Ui, if (step.key || on) colors.text else colors.textMuted)

            if (step.key) {
                Tag("Key")
            }
        }

        if (onToggleReviewed != null) {
            Box(
                Modifier
                    .size(touchMin(Tokens.Size.ControlSm))
                    .pressable(onToggleReviewed, role = Role.Checkbox, description = "Reviewed: ${step.title}", shape = ControlShape)
                    .semantics { toggleableState = if (reviewed) ToggleableState.On else ToggleableState.Off },
                contentAlignment = Alignment.Center,
            ) {
                Icon(if (reviewed) MyIcon.CircleCheck else MyIcon.Circle, if (reviewed) colors.text else colors.textMuted)
            }
        }
    }
}

@Composable
private fun NumberChip(position: Int, on: Boolean) {
    val colors = MyTheme.colors

    Box(
        Modifier
            .size(Tokens.Size.ControlSm)
            .background(if (on) colors.text else Color.Transparent, CircleShape)
            .border(Tokens.Size.Hairline, if (on) colors.text else colors.textMuted, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        Label("${position + 1}", style = Tokens.Type.Meta, color = if (on) colors.bgContent else colors.textMuted)
    }
}

@Composable
private fun StepDetail(
    step: WalkthroughStep,
    index: Int,
    walkthrough: Walkthrough,
    notes: List<WalkthroughNote>,
    annotatable: Boolean,
    approved: Boolean,
    onApprove: (() -> Unit)?,
    onAnnotate: (WalkthroughFile, FileDiff, Annotation) -> Unit,
    onNote: (String) -> Unit,
    onRemoveNote: (WalkthroughNote) -> Unit,
) {
    val colors = MyTheme.colors
    val outer = LocalAnnotate.current
    val annotate by rememberUpdatedState(onAnnotate)

    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        NumberChip(index, true)

        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label(step.title, Modifier.weight(1f, fill = false).semantics { heading() }, Tokens.Type.Title)

            if (step.key) {
                Tag("Key change")
            }
        }

        if (onApprove != null) {
            MyButton(onApprove, text = if (approved) "Approved" else "Approve", icon = if (approved) MyIcon.CircleCheck else MyIcon.Circle, variant = ButtonVariant.Outline, size = ButtonSize.Sm, selected = approved)
        }
    }

    if (step.explanation.isNotBlank()) {
        Label(step.explanation, color = colors.textMuted)
    }

    for (file in step.files) {
        if (file.unchanged) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                Label(file.path, style = Tokens.Type.UiStrong)
                Label("No changes to this file in this scope", color = colors.textMuted)
            }

            continue
        }

        val diff = walkthroughDiff(file)

        if (!annotatable) {
            DiffCard(diff, ToolStatus.Done)

            continue
        }

        val actions = remember(index, file.path) { AnnotateActions(onAttach = { annotate(file, diff, it) }, onAsk = outer?.onAsk ?: { _, _ -> }, withComment = true) }

        CompositionLocalProvider(LocalAnnotate provides actions) {
            DiffCard(diff, ToolStatus.Done)
        }
    }

    if (!annotatable) {
        return
    }

    if (notes.isNotEmpty()) {
        Label("Notes", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        for (note in notes) {
            val label = walkthroughNoteLabel(walkthrough, note)

            Row(Modifier.fillMaxWidth().semantics(mergeDescendants = true) { contentDescription = "Note: $label" }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                Label(label, Modifier.weight(1f))
                MyButton({ onRemoveNote(note) }, icon = MyIcon.Close, description = "Remove note: $label", size = ButtonSize.Sm, muted = true)
            }
        }
    }

    var draft by remember(index) { mutableStateOf("") }
    var error by remember(index) { mutableStateOf<String?>(null) }

    fun add() {
        if (draft.isBlank()) {
            error = "Enter a note to add to this step."

            return
        }

        onNote(draft.trim())
        draft = ""
        error = null
    }

    TextInput(draft, { draft = it; error = null }, label = "Note on this step", placeholder = "Add a note for the agent", error = error, onSubmit = ::add)
    MyButton(::add, text = "Add note", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
}
