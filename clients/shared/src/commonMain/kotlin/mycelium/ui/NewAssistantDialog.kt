package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.CreateAssistantRequest
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.NoGroup
import mycelium.state.Store

@Composable
fun NewAssistantDialog(store: Store, state: AppState, onDismiss: () -> Unit, onCreated: (mycelium.api.Assistant) -> Unit) {
    val colors = MyTheme.colors
    var name by remember { mutableStateOf("") }
    var group by remember { mutableStateOf(NoGroup) }
    var instructions by remember { mutableStateOf("") }
    var nameError by remember { mutableStateOf<String?>(null) }
    val runner = rememberAction()
    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()
    val groupChoices = state.groups.values.sortedBy { it.name.lowercase() }.map { Choice(it.id, it.name) } + Choice(NoGroup, "No group")

    LaunchedEffect(Unit) {
        store.load { store.refreshGroups() }
    }

    fun create() {
        if (name.isBlank()) {
            nameError = "Enter a name for the assistant."

            return
        }

        runner {
            val created = store.createAssistant(CreateAssistantRequest(name.trim(), null, group.takeUnless { it == NoGroup }, instructions.ifBlank { null }))

            onCreated(created)
        }
    }

    Dialog(onDismiss) {
        Label("New assistant", Modifier.semantics { heading() }, Tokens.Type.Title)

        TextInput(name, { name = it; nameError = null }, label = "Name", placeholder = "Stand-up", error = nameError, onSubmit = ::create, modifier = Modifier.fillMaxWidth())

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Group", style = Tokens.Type.MetaStrong, color = colors.textMuted)
            OptionMenu("Group", groupChoices, group, { group = it }, title = "Group", icon = MyIcon.Group)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Instructions", style = Tokens.Type.MetaStrong, color = colors.textMuted)

            BasicTextField(
                value = instructions,
                onValueChange = { instructions = it },
                interactionSource = source,
                textStyle = Tokens.Type.Ui.themed().copy(color = colors.text),
                cursorBrush = SolidColor(colors.text),
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 120.dp)
                    .focusRing(focused, ControlShape, always = true)
                    .background(colors.bgContent, ControlShape)
                    .border(Tokens.Size.Hairline, colors.borderControl, ControlShape)
                    .padding(Tokens.Space.S2)
                    .semantics { contentDescription = "Instructions" },
            )

            Label("What it is for and how it should work. You can change this later.", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        Label("It starts on your default model, on any node that is online. Set models, nodes, storage, secrets, actions and schedules in its settings afterwards.", style = Tokens.Type.Meta, color = colors.textMuted)

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::create, text = "Create assistant", variant = ButtonVariant.Outline)
        }
    }
}
