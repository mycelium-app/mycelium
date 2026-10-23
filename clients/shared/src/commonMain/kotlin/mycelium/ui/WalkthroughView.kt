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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.Walkthrough
import mycelium.state.ToolStatus
import mycelium.state.stepPosition
import mycelium.state.walkthroughDiff

@Composable
fun WalkthroughView(walkthrough: Walkthrough, modifier: Modifier = Modifier) {
    var selected by remember(walkthrough.id) { mutableStateOf(0) }
    val last = walkthrough.steps.lastIndex
    val index = selected.coerceIn(0, maxOf(last, 0))
    val steps: @Composable () -> Unit = {
        for ((position, step) in walkthrough.steps.withIndex()) {
            StepRow(position, step.title, step.key, position == index) { selected = position }
        }
    }
    val detail: @Composable () -> Unit = {
        if (walkthrough.steps.isNotEmpty()) {
            StepDetail(walkthrough, index, onPrevious = { selected = index - 1 }, onNext = { selected = index + 1 })
        }
    }

    if (LocalTouch.current) {
        Column(modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Column(Modifier.semantics { contentDescription = "Walkthrough steps" }) { steps() }
            Hairline()
            detail()
        }

        return
    }

    Row(modifier.fillMaxSize()) {
        Column(Modifier.width(Tokens.Size.Sidebar).fillMaxHeight().verticalScroll(rememberScrollState()).padding(Tokens.Space.S2).semantics { contentDescription = "Walkthrough steps" }) {
            steps()
        }

        VerticalHairline()

        Column(Modifier.weight(1f).fillMaxHeight().verticalScroll(rememberScrollState()).padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            detail()
        }
    }
}

@Composable
private fun StepRow(position: Int, title: String, key: Boolean, on: Boolean, onClick: () -> Unit) {
    val colors = MyTheme.colors

    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = rowHeight())
            .pressable(onClick, role = Role.Tab, description = "Step ${position + 1}: $title${if (key) ", key step" else ""}", shape = ControlShape, selected = on)
            .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(Tokens.Size.ControlSm)
                .background(if (on) colors.text else Color.Transparent, CircleShape)
                .border(Tokens.Size.Hairline, if (on) colors.text else colors.textMuted, CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            Label("${position + 1}", style = Tokens.Type.Meta, color = if (on) colors.bgContent else colors.textMuted)
        }

        Label(title, Modifier.weight(1f), if (key || on) Tokens.Type.UiStrong else Tokens.Type.Ui, if (key || on) colors.text else colors.textMuted)

        if (key) {
            Tag("Key")
        }
    }
}

@Composable
private fun StepDetail(walkthrough: Walkthrough, index: Int, onPrevious: () -> Unit, onNext: () -> Unit) {
    val colors = MyTheme.colors
    val step = walkthrough.steps[index]

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        Label(step.title, Modifier.semantics { heading() }, Tokens.Type.Title)

        if (step.key) {
            Tag("Key step")
        }
    }

    if (step.explanation.isNotBlank()) {
        Label(step.explanation, color = colors.textMuted)
    }

    for (file in step.files) {
        DiffCard(walkthroughDiff(file), ToolStatus.Done)
    }

    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        if (index > 0) {
            MyButton(onPrevious, text = "Previous step", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        Label(stepPosition(index, walkthrough.steps.size), style = Tokens.Type.Meta, color = colors.textMuted)

        if (index < walkthrough.steps.lastIndex) {
            MyButton(onNext, text = "Next step", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }
    }
}
