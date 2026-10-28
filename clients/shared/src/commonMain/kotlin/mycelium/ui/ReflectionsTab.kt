package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.background
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.Platform
import mycelium.api.Reflection
import mycelium.state.LearningState
import mycelium.state.ReflectionSection
import mycelium.state.Store
import mycelium.state.draftedLabel
import mycelium.state.text
import mycelium.state.wasEdited
import mycelium.state.weekLabel

private val SectionHeight = 72.dp

@Composable
fun ReflectionsTab(store: Store, platform: Platform, onOpenAssistant: (String) -> Unit) {
    val learning by store.learning.state.collectAsState()
    val app by store.state.collectAsState()
    val wellnessWakes = app.assistants.values.firstOrNull { it.builtinKey == "wellness" }?.wakes

    LaunchedEffect(Unit) {
        store.refreshAssistants()
        store.learning.load()
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        learning.problem?.let { Label(it, Modifier.semantics { liveRegion = LiveRegionMode.Assertive }, Tokens.Type.MetaStrong, MyTheme.colors.statusFailed) }

        Label(
            wellnessWakes?.let { "$it, the Wellness assistant drafts this from the week's conversations and your break notes." } ?: "The Wellness assistant drafts this from the week's conversations and your break notes.",
            color = MyTheme.colors.textMuted,
        )
        MyButton({ store.act { onOpenAssistant(store.api.wellnessAssistant().id) } }, text = "Open the Wellness assistant", variant = ButtonVariant.Outline)

        if (learning.reflections.isNotEmpty()) {
            Label("Redraft rewrites only the sections you have not edited.", color = MyTheme.colors.textMuted)
        }

        for (reflection in learning.reflections) {
            ReflectionCard(store, platform, reflection, learning)
        }
    }
}

@Composable
private fun ReflectionCard(store: Store, platform: Platform, reflection: Reflection, learning: LearningState) {
    val colors = MyTheme.colors
    val heading = weekLabel(reflection.week)
    val drafting = reflection.id in learning.redrafting
    var started by remember(reflection.id) { mutableStateOf(false) }

    Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, ControlShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label(heading, Modifier.weight(1f).semantics { this.heading() }, Tokens.Type.Title)
            reflection.draftedAt?.let { drafted -> draftedLabel(drafted, platform::localDateTime)?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted) } }
            MyButton(
                { started = true; store.learning.redraft(reflection) },
                text = if (drafting) "Drafting" else "Redraft",
                description = "Redraft the ${heading.replaceFirstChar { it.lowercase() }}",
                variant = ButtonVariant.Outline,
                size = ButtonSize.Sm,
            )
        }

        if (started && !drafting && learning.problem == null) {
            Label("Drafting has started. The new text appears here when the Wellness assistant saves it.", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
        }

        for (section in ReflectionSection.entries) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    Label(section.title, style = Tokens.Type.MetaStrong, color = colors.textMuted)

                    if (reflection.wasEdited(section)) {
                        Tag("Edited")
                    }
                }

                SectionField("${section.title}, ${heading.replaceFirstChar { it.lowercase() }}", reflection.text(section)) { store.learning.save(reflection, section, it) }
            }
        }
    }
}

@Composable
private fun SectionField(label: String, value: String, onCommit: (String) -> Unit) {
    val colors = MyTheme.colors
    var text by remember(value) { mutableStateOf(value) }
    var wasFocused by remember { mutableStateOf(false) }
    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()

    BasicTextField(
        value = text,
        onValueChange = { text = it },
        interactionSource = source,
        textStyle = Tokens.Type.Ui.themed().copy(color = colors.text),
        cursorBrush = SolidColor(colors.text),
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = SectionHeight)
            .onFocusChanged { state ->
                if (wasFocused && !state.isFocused) {
                    onCommit(text)
                }

                wasFocused = state.isFocused
            }
            .focusRing(focused, ControlShape, always = true)
            .background(colors.bgContent, ControlShape)
            .border(Tokens.Size.Hairline, colors.borderControl, ControlShape)
            .padding(Tokens.Space.S2)
            .semantics { contentDescription = label },
    )
}
