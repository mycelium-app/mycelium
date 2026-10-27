package mycelium.ui

import androidx.compose.runtime.Composable

@Composable
fun DeleteConversationDialog(title: String, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    ConfirmDialog("Delete $title?", "Its messages and attachments are removed. This cannot be undone.", "Delete conversation", onDismiss, onConfirm)
}
