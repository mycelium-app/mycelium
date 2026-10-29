package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.state.chatToolName

class SessionPermissions(val granted: List<String>, val onRevoke: ((String) -> Unit)?)

@Composable
fun SessionPermissionsDialog(permissions: SessionPermissions, onDismiss: () -> Unit) {
    Dialog(onDismiss = onDismiss) {
        Label("Permissions for this session", Modifier.semantics { heading() }, Tokens.Type.Title)

        if (permissions.granted.isEmpty()) {
            Label("Nothing approved yet.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        }

        for (title in permissions.granted) {
            val name = chatToolName(title)?.tool ?: title

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label(name, Modifier.weight(1f), Tokens.Type.Code, maxLines = 1)

                permissions.onRevoke?.let { revoke ->
                    MyButton({ revoke(title) }, text = "Revoke", variant = ButtonVariant.Outline, size = ButtonSize.Sm, description = "Revoke $name")
                }
            }
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            MyButton(onDismiss, text = "Close", muted = true)
        }
    }
}
