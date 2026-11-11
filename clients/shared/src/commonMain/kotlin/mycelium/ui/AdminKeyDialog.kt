package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.api.ModelProvider
import mycelium.api.UpdateModelProviderRequest
import mycelium.state.AdminKeyPrefix
import mycelium.state.AdminKeyProblem
import mycelium.state.Store

@Composable
fun AdminKeyDialog(store: Store, provider: ModelProvider, onSaved: (ModelProvider) -> Unit, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    var key by remember(provider.id) { mutableStateOf("") }
    var entryProblem by remember(provider.id) { mutableStateOf<String?>(null) }
    val keyFocus = remember { FocusRequester() }
    val runner = rememberAction(provider.id)
    val problem = entryProblem ?: runner.failure

    fun save() {
        if (!key.trim().startsWith(AdminKeyPrefix)) {
            entryProblem = AdminKeyProblem

            return
        }

        entryProblem = null
        runner.failure = null
        runner {
            onSaved(store.api.updateModelProvider(provider.id, UpdateModelProviderRequest(adminKey = key.trim())))
            onDismiss()
        }
    }

    Dialog(onDismiss) {
        Label("Admin API key for ${provider.name}", Modifier.semantics { heading() }, Tokens.Type.Title)

        problem?.let { found ->
            Column(
                Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.statusFailed, ControlShape).padding(Tokens.Space.S3).semantics { liveRegion = LiveRegionMode.Assertive },
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            ) {
                Label("There is a problem", Modifier.semantics { heading() }, Tokens.Type.UiStrong, colors.statusFailed)
                Label(found, Modifier.clickable(role = Role.Button) { runCatching { keyFocus.requestFocus() } }, Tokens.Type.Ui, colors.statusFailed, decoration = LabelDecoration.Underline)
            }
        }

        Label("Mycelium uses this key only to read cost and token usage for the Usage card. Chats keep using the API key.", style = Tokens.Type.Meta, color = colors.textMuted)

        TextInput(key, { key = it }, label = "Admin API key (optional)", hint = "Create one in the Anthropic Console under Settings, Admin keys. It starts with $AdminKeyPrefix.", error = problem, errorAbove = true, mono = true, masked = true, onSubmit = ::save, focusRequester = keyFocus)

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            if (provider.hasAdminKey) {
                MyButton({ runner { onSaved(store.api.updateModelProvider(provider.id, UpdateModelProviderRequest(adminKey = ""))); onDismiss() } }, text = "Remove admin key", muted = true)
            }

            Spacer(Modifier.weight(1f))
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Primary)
        }
    }
}
