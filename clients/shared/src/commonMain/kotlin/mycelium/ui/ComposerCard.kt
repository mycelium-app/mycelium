package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import mycelium.api.ModelProvider

class ModelPickerControls(
    val providers: List<ModelProvider>,
    val providerId: String?,
    val model: String?,
    val onSelect: (ModelProvider, String) -> Unit,
)

@Composable
fun ModelPicker(controls: ModelPickerControls) {
    val current = controls.providers.firstOrNull { it.id == controls.providerId }
    val label = if (current != null && controls.model != null) "${current.name} · ${controls.model}" else "Choose a model"
    var open by remember { mutableStateOf(false) }

    Box {
        MyButton({ open = true }, text = label, description = "Model: $label", muted = true, trailingIcon = MyIcon.ChevronDown, selected = open, tooltip = "Model")

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                Column(Modifier.heightIn(max = Tokens.Size.MenuList).verticalScrollWithBar()) {
                    for (provider in controls.providers.filter { it.models.isNotEmpty() }) {
                        MenuLabel(provider.name)

                        for (model in provider.models) {
                            MenuItem(model, onClick = { controls.onSelect(provider, model); open = false }, checked = provider.id == controls.providerId && model == controls.model)
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun ComposerCard(
    value: TextFieldValue,
    onValueChange: (TextFieldValue) -> Unit,
    placeholder: String,
    onSubmit: () -> Unit,
    attachments: AttachmentControls,
    modifier: Modifier = Modifier,
    error: String? = null,
    trailing: @Composable RowScope.() -> Unit = {},
) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val shape = RoundedCornerShape(Tokens.Radius.Card)
    val source = remember { MutableInteractionSource() }

    DisposableEffect(platform, attachments.onAdd) {
        platform?.listenForFiles(attachments.onAdd)

        onDispose { platform?.listenForFiles(null) }
    }

    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Column(
            Modifier
                .fillMaxWidth()
                .background(colors.bgRaised, shape)
                .border(Tokens.Size.Hairline, colors.border, shape)
                .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
        ) {
            AttachmentChips(attachments)

            MessageField(
                text = value,
                onTextChange = onValueChange,
                source = source,
                onSubmit = onSubmit,
                onPaste = {
                    val pasted = platform?.clipboardFiles().orEmpty()

                    attachments.onAdd(pasted)
                    pasted.isNotEmpty()
                },
                onKey = { false },
                modifier = Modifier.heightIn(min = 88.dp).padding(top = Tokens.Space.S1),
                alignment = Alignment.TopStart,
                placeholder = placeholder,
            )

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                if (platform != null) {
                    MyButton({ platform.pickFiles(attachments.onAdd) }, icon = MyIcon.Paperclip, description = "Attach files", muted = true)
                }

                Box(Modifier.weight(1f))
                trailing()
                MyButton(onSubmit, icon = MyIcon.Send, description = "Send", muted = true)
            }
        }

        if (error != null) {
            Row(Modifier.semantics { liveRegion = LiveRegionMode.Assertive }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                Icon(MyIcon.Failed, colors.statusFailed, Tokens.Size.IconSm)
                Label(error, style = Tokens.Type.Meta, color = colors.statusFailed)
            }
        }
    }
}
