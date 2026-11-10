package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.state.agentName
import mycelium.state.relativeTime
import mycelium.state.buildThread
import mycelium.state.statusSectionName
import mycelium.state.threadSubagentsOf

private val SubagentListMaxHeight = Tokens.Size.Row * 5
private val SubagentPlaceWidth = 184.dp
private val SubagentStatusWidth = 76.dp
private val SubagentAgeWidth = 32.dp

class ThreadSubagents(val store: Store, val state: AppState, val conversationId: String, val onOpen: (String) -> Unit)

val LocalThreadSubagents = compositionLocalOf<ThreadSubagents?> { null }

@Composable
fun SubagentCards(actions: ThreadSubagents) {
    val subagents = threadSubagentsOf(actions.state.conversations.values, actions.conversationId)

    if (subagents.isEmpty()) {
        return
    }

    var expanded by remember { mutableStateOf(true) }
    val colors = MyTheme.colors
    val waiting = subagents.filter { it.status == ConversationStatus.Input }
    val count = "${subagents.size} ${if (subagents.size == 1) "subagent" else "subagents"}"

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Card(Modifier.semantics { contentDescription = "Subagents" }) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .pressable({ expanded = !expanded }, description = "$count, ${if (expanded) "hide" else "show"} them")
                    .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (waiting.isNotEmpty()) {
                    Icon(MyIcon.Alert, colors.statusAttention, Tokens.Size.IconSm)
                }

                Label(count, Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
                Icon(if (expanded) MyIcon.ChevronDown else MyIcon.ChevronRight, colors.textMuted, Tokens.Size.IconSm)
            }

            if (expanded) {
                Column(Modifier.fillMaxWidth().heightIn(max = SubagentListMaxHeight).verticalScrollWithBar()) {
                    for (subagent in subagents) {
                        val status = statusSectionName(subagent.status)
                        val place = listOfNotNull(subagent.model?.ifEmpty { null }, actions.state.nodes[subagent.nodeId]?.name?.let { "on $it" }).joinToString(" ")
                        val age = relativeTime(subagent.createdAt)

                        Row(
                            Modifier
                                .fillMaxWidth()
                                .pressable({ actions.onOpen(subagent.id) }, description = listOf(subagent.title, agentName(subagent.agent).substringBefore(" "), place, status, age).filter { it.isNotEmpty() }.joinToString(", "))
                                .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1_5),
                            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(iconOf(subagent.status), toneOf(subagent.status).color(), Tokens.Size.IconSm)
                            Label(subagent.title, Modifier.weight(1f), maxLines = 1)
                            Icon(agentIcon(subagent.agent), colors.textMuted, Tokens.Size.IconSm)
                            Label(place, Modifier.width(SubagentPlaceWidth), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                            Label(status, Modifier.width(SubagentStatusWidth), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                            Label(age, Modifier.width(SubagentAgeWidth), Tokens.Type.Meta.copy(textAlign = TextAlign.End), colors.textFaint, maxLines = 1)
                        }
                    }
                }
            }
        }

        for (subagent in waiting) {
            WaitingSubagentCard(actions, subagent)
        }
    }
}

@Composable
private fun WaitingSubagentCard(actions: ThreadSubagents, subagent: Conversation) {
    val events = actions.state.events[subagent.id]
    val question = remember(events) { buildThread(events.orEmpty()).filterIsInstance<ThreadItem.AgentMessage>().lastOrNull() }
    val colors = MyTheme.colors

    LaunchedEffect(subagent.id) {
        ensureOpened(actions.store, subagent.id)
    }

    Card(Modifier.semantics { contentDescription = "${subagent.title}, waiting for your answer" }) {
        CardHead {
            Icon(agentIcon(subagent.agent), colors.textMuted, Tokens.Size.IconSm)
            Label(subagent.title, Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 1)
        }

        Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            if (question != null) {
                Label(question.text)
            }

            WaitingForAnswerLine()
            ReplyField("Reply to ${subagent.title}", "Send to ${subagent.title}", { text -> actions.store.send(subagent.id, text) }, placeholder = "Reply to this subagent", showLabel = false, sendText = "Send")
        }
    }
}
