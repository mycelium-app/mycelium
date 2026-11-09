package mycelium.ui

import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.state.buildThread
import mycelium.state.projectName
import mycelium.state.relativeTime
import mycelium.state.statusSectionName

class BroughtInActions(val store: Store, val state: AppState, val onOpen: (String) -> Unit)

val LocalBroughtIn = compositionLocalOf<BroughtInActions?> { null }

private const val ShownMessages = 4
private const val ShownMessageLines = 6

@Composable
fun BroughtInConversation(actions: BroughtInActions, conversationId: String, title: String) {
    val store = actions.store
    val state = actions.state
    val conversation = state.conversations[conversationId]
    val events = state.events[conversationId]
    val thread = remember(events) { buildThread(events.orEmpty()) }
    val colors = MyTheme.colors
    var repliedAfter by remember { mutableStateOf<Int?>(null) }

    LaunchedEffect(conversationId) {
        ensureOpened(store, conversationId)
    }

    if (conversation == null) {
        Label("The chat $title is not available.", style = Tokens.Type.Meta, color = colors.textMuted)

        return
    }

    val project = conversation.projectId?.let { state.projects[it] }
    val assistant = conversation.assistantId?.let { state.assistants[it] }
    val place = project?.name ?: projectName(conversation.cwd)
    val label = "$place, ${conversation.title}"
    val requests = pendingRequests(thread, state).map { it.request } + workflowGates(state).filter { it.conversationId == conversationId }.map { InboxRequest.Gate(it) }
    val exchanged = thread.filter { it is ThreadItem.UserMessage || it is ThreadItem.AgentMessage }
    val shown = repliedAfter?.let { exchanged.drop(it).takeLast(ShownMessages) } ?: exchanged.takeLast(1).filterIsInstance<ThreadItem.AgentMessage>()

    Card(Modifier.semantics { contentDescription = "Chat: $label" }) {
        CardHead {
            if (assistant != null) {
                AssistantAvatar(assistant)
            } else if (project != null) {
                ProjectIcon(project)
            }

            val heading = buildAnnotatedString {
                if (assistant == null && project != null) {
                    withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(project.name) }
                    append(" ")
                }

                append(conversation.title)
            }
            BasicText(heading, Modifier.weight(1f), style = Tokens.Type.Ui.themed().copy(color = colors.text), maxLines = 1, overflow = TextOverflow.Ellipsis)
            StatusMark(statusSectionName(conversation.status), toneOf(conversation.status), iconOf(conversation.status))
            Label(relativeTime(conversation.updatedAt), style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
            MyButton({ actions.onOpen(conversationId) }, text = "Open chat", trailingIcon = MyIcon.Move, description = "Open the chat $label", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        Column(Modifier.padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            for (message in shown) {
                when (message) {
                    is ThreadItem.UserMessage -> UserMessage(message.text, message.files, message.images, message.goalSentAt, message.goalNudge, message.goalWhy, message.spoken, message.spokenAt, conversation.agent)
                    is ThreadItem.AgentMessage -> {
                        val tone = if (repliedAfter == null) colors.textMuted else colors.text

                        BasicText(inlineText(message.text, tone), style = Tokens.Type.Ui.themed().copy(color = tone), maxLines = ShownMessageLines, overflow = TextOverflow.Ellipsis)
                    }
                    else -> Unit
                }
            }

            for (request in requests) {
                RequestCard(store, state, conversationId, request)
            }
        }

        Hairline()

        Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)) {
            ReplyField("Reply in ${conversation.title}", "Send to ${conversation.title}", { text ->
                repliedAfter = exchanged.size
                store.send(conversationId, text)
            }, Modifier.padding(bottom = Tokens.Space.S1), placeholder = "Message the chat", hint = "Sent as you, straight into the chat")
        }
    }
}
