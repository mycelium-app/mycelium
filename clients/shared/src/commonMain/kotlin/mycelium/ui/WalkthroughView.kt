package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
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
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.delay
import mycelium.api.Walkthrough
import mycelium.api.WalkthroughFile
import mycelium.api.WalkthroughStep
import mycelium.state.Store
import mycelium.state.ToolStatus
import mycelium.state.WalkthroughScope
import mycelium.state.stepPosition
import mycelium.state.walkthroughDiff
import mycelium.state.walkthroughSections

private const val ReadAfterMillis = 1_500L
private const val NotCoveredTitle = "Not covered yet"

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
    var reads by remember(walkthrough.id) { mutableStateOf(emptySet<Int>()) }
    val scopes by store.walkthroughScopes.collectAsState()
    var uncovered by remember(walkthrough.id) { mutableStateOf(emptyList<WalkthroughFile>()) }
    val steps = walkthrough.steps + listOfNotNull(WalkthroughStep(NotCoveredTitle, section = "Not covered", explanation = "These changes are not in any step above.", files = uncovered).takeIf { uncovered.isNotEmpty() })
    val last = steps.lastIndex
    val index = selected.coerceIn(0, maxOf(last, 0))

    fun markRead(step: Int) {
        if (step in reads || step >= walkthrough.steps.size) {
            return
        }

        reads = reads + step
        store.act { store.api.markWalkthroughRead(walkthrough.id, step) }
    }

    LaunchedEffect(walkthrough.id) {
        val stored = runCatching { store.api.walkthroughReads(walkthrough.id) }.getOrDefault(emptyList())
        reads = reads + stored
    }

    LaunchedEffect(walkthrough.id) {
        uncovered = runCatching { store.api.walkthroughUncovered(walkthrough.id) }.getOrDefault(emptyList())
    }

    LaunchedEffect(walkthrough.id, index) {
        delay(ReadAfterMillis)
        markRead(index)
    }

    fun move(to: Int) {
        selected = to.coerceIn(0, maxOf(last, 0))
    }

    fun next() {
        markRead(index)
        move(index + 1)
    }

    val stepList: @Composable () -> Unit = {
        for (section in walkthroughSections(steps)) {
            if (section.name != null) {
                SectionHeader(section.name, sectionIcon(section.icon))
            }

            for ((position, step) in section.steps) {
                StepRow(position, step, position == index, position in reads) { selected = position }
            }
        }
    }
    val detail: @Composable () -> Unit = {
        if (steps.isNotEmpty()) {
            walkthrough.note?.let { Label(it, color = MyTheme.colors.textMuted) }
            StepDetail(steps[index], index)
        }
    }
    val header: @Composable () -> Unit = {
        WalkthroughHeader(
            scope = scopes[conversationId] ?: WalkthroughScope.Branch,
            position = stepPosition(index, steps.size),
            onPrevious = index.takeIf { it > 0 }?.let { { move(index - 1) } },
            onNext = index.takeIf { it < last }?.let { { next() } },
            onScope = { store.chooseWalkthroughScope(conversationId, it) },
            onRegenerate = { store.regenerateWalkthrough(conversationId) },
            onDismiss = { store.setWalkthroughDismissed(conversationId, true) },
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

    if (LocalTouch.current) {
        Column(modifier.fillMaxSize().verticalScrollWithBar().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            header()
            Column(stepKeys.semantics { contentDescription = "Walkthrough steps" }) { stepList() }
            Hairline()
            detail()
        }

        return
    }

    Column(modifier.fillMaxSize()) {
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

@Composable
private fun WalkthroughHeader(scope: WalkthroughScope, position: String, onPrevious: (() -> Unit)?, onNext: (() -> Unit)?, onScope: (WalkthroughScope) -> Unit, onRegenerate: () -> Unit, onDismiss: () -> Unit) {
    var open by remember { mutableStateOf(false) }

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
        MyButton({ onRegenerate() }, text = "Regenerate", icon = MyIcon.Refresh, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        MyButton({ onDismiss() }, text = "Dismiss", icon = MyIcon.Close, description = "Dismiss walkthrough", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
    }
}

@Composable
private fun SectionHeader(name: String, icon: MyIcon) {
    val colors = MyTheme.colors

    Row(
        Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5).semantics(mergeDescendants = true) { heading() },
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, colors.textMuted, Tokens.Size.IconSm)
        Label(name.uppercase(), style = Tokens.Type.Meta, color = colors.textMuted)
    }
}

@Composable
private fun StepRow(position: Int, step: WalkthroughStep, on: Boolean, read: Boolean, onClick: () -> Unit) {
    val colors = MyTheme.colors
    val notes = listOfNotNull("key change".takeIf { step.key }, "viewed".takeIf { read })

    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = rowHeight())
            .pressable(onClick, role = Role.Tab, description = (listOf("Step ${position + 1}: ${step.title}") + notes).joinToString(", "), shape = ControlShape, selected = on)
            .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        NumberChip(position, on)

        Label(step.title, Modifier.weight(1f), if (step.key || on) Tokens.Type.UiStrong else Tokens.Type.Ui, if (step.key || on) colors.text else colors.textMuted)

        if (step.key) {
            Tag("Key")
        }

        if (read) {
            Icon(MyIcon.Check, colors.textMuted, Tokens.Size.IconSm)
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
private fun StepDetail(step: WalkthroughStep, index: Int) {
    val colors = MyTheme.colors

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        NumberChip(index, true)
        Label(step.title, Modifier.weight(1f, fill = false).semantics { heading() }, Tokens.Type.Title)

        if (step.key) {
            Tag("Key change")
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
        } else {
            DiffCard(walkthroughDiff(file), ToolStatus.Done)
        }
    }
}
