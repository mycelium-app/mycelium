package mycelium.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Dp
import mycelium.state.AppState
import mycelium.state.Store

@Composable
fun BesideChat(
    store: Store,
    state: AppState,
    subjectKey: String,
    subject: String,
    subjectTitle: String,
    conversationId: String?,
    onArchived: () -> Unit,
    onOpenNodes: () -> Unit,
    onOpenConversation: (String) -> Unit,
    viewer: @Composable (Modifier, @Composable RowScope.() -> Unit) -> Unit,
) {
    val attached = conversationId?.takeIf { state.conversations[it] != null }
    val compact = LocalTouch.current
    var chatShown by remember(subjectKey) { mutableStateOf(attached != null && !compact) }
    var chatWidth by remember { mutableStateOf(Tokens.Size.SidePanel) }

    fun resizeChat(width: Dp) {
        chatWidth = width.coerceIn(Tokens.Size.SidePanelMin, Tokens.Size.SidePanelMax)
    }

    val toggle: @Composable RowScope.() -> Unit = {
        if (attached != null) {
            MyButton({ chatShown = !chatShown }, icon = MyIcon.Chat, description = if (chatShown) "Hide chat" else "Show chat", muted = true, selected = chatShown)
        }
    }

    Row(Modifier.fillMaxSize()) {
        if (!(compact && chatShown)) {
            viewer(Modifier.weight(1f).fillMaxHeight(), toggle)
        }

        if (attached != null && chatShown) {
            if (compact) {
                Column(Modifier.weight(1f).fillMaxHeight()) {
                    Bar {
                        BackButton(subject, { chatShown = false })
                        Label(subjectTitle, style = Tokens.Type.Title, maxLines = 1)
                    }

                    ConversationScreen(store, state, attached, onArchived = onArchived, onOpenNodes = onOpenNodes, onOpenConversation = onOpenConversation, panels = false)
                }
            } else {
                ResizeHandle("Resize the chat") { moved -> resizeChat(chatWidth - moved) }

                Column(Modifier.width(chatWidth).fillMaxHeight()) {
                    ConversationScreen(store, state, attached, onArchived = onArchived, onOpenNodes = onOpenNodes, onOpenConversation = onOpenConversation, panels = false)
                }
            }
        }
    }
}
