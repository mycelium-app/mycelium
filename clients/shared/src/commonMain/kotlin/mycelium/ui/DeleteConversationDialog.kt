package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics

@Composable
fun DeleteConversationDialog(title: String, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    Dialog(onDismiss = onDismiss) {
        Label("Delete $title?", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label("Its messages and attachments are removed. This cannot be undone.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(onConfirm, text = "Delete conversation", variant = ButtonVariant.Danger)
        }
    }
}
