package mycelium.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.state.AppState
import mycelium.state.Store

private const val BesideChatPreference = "beside-chat"

private const val BesideChatPickerRows = 30

private val BesideChatPickerWidth = 340.dp

private val BesideChatPickerHeight = 320.dp

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
    val platform = LocalPlatform.current
    val pickedKey = "$BesideChatPreference:$subjectKey"
    var picked by remember(subjectKey) { mutableStateOf(platform?.loadPreference(pickedKey)?.takeIf { it.isNotEmpty() }) }
    val attached = (conversationId ?: picked)?.takeIf { state.conversations[it] != null }
    val compact = LocalTouch.current
    var chatShown by remember(subjectKey) { mutableStateOf(attached != null && !compact) }
    var picking by remember(subjectKey) { mutableStateOf(false) }
    var query by remember(subjectKey) { mutableStateOf("") }
    var chatWidth by remember { mutableStateOf(Tokens.Size.SidePanel) }

    fun resizeChat(width: Dp) {
        chatWidth = width.coerceIn(Tokens.Size.SidePanelMin, Tokens.Size.SidePanelMax)
    }

    val toggle: @Composable RowScope.() -> Unit = {
        Box {
            MyButton(
                { if (attached != null) chatShown = !chatShown else picking = true },
                icon = MyIcon.Chat,
                description = if (attached != null && chatShown) "Hide chat" else "Show chat",
                muted = true,
                selected = attached != null && chatShown,
            )

            if (picking) {
                val matches = state.conversations.values
                    .filter { it.archivedAt == null && it.parentId == null && it.assistantId == null && it.title.contains(query.trim(), ignoreCase = true) }
                    .sortedByDescending { it.updatedAt }
                    .take(BesideChatPickerRows)

                MenuPopup(onDismiss = { picking = false }, width = BesideChatPickerWidth) {
                    MenuLabel("Show a conversation beside this")
                    ListSearch(query, { query = it }, "conversations", Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1))

                    Column(Modifier.heightIn(max = BesideChatPickerHeight).verticalScroll(rememberScrollState())) {
                        if (matches.isEmpty()) {
                            Label("No conversations found.", Modifier.padding(Tokens.Space.S3), color = MyTheme.colors.textMuted)
                        }

                        for (row in matches) {
                            MenuItem(
                                row.title,
                                onClick = {
                                    picked = row.id
                                    platform?.savePreference(pickedKey, row.id)
                                    chatShown = true
                                    picking = false
                                },
                                leading = MyIcon.Chat,
                            )
                        }
                    }
                }
            }
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
