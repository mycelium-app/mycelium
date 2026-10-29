package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.state.isChat

enum class ConversationKind {
    WorkspaceIdle,
    WorkspaceWorking,
    WorkspaceFailed,
    ChatOnAgent,
    ChatOnModel,
    Assistant,
    NewConversation,
    NewChat,
}

enum class BarGroup { Tools, Pickers, Usage, Turn }

enum class BarControl(val group: BarGroup) {
    Attach(BarGroup.Tools),
    Project(BarGroup.Tools),
    Place(BarGroup.Tools),
    Actions(BarGroup.Tools),
    Dictate(BarGroup.Tools),
    TeachMe(BarGroup.Tools),
    Goal(BarGroup.Tools),
    Account(BarGroup.Pickers),
    Model(BarGroup.Pickers),
    Mode(BarGroup.Pickers),
    Settings(BarGroup.Pickers),
    Usage(BarGroup.Usage),
    Stop(BarGroup.Turn),
    Send(BarGroup.Turn),
}

data class HarnessReport(
    val voice: Boolean = false,
    val teachMe: Boolean = false,
    val working: Boolean = false,
    val models: Boolean = false,
    val modes: Boolean = false,
    val settings: Boolean = false,
)

fun conversationKind(conversation: Conversation, working: Boolean): ConversationKind = when {
    conversation.assistantId != null -> ConversationKind.Assistant
    conversation.isChat -> ConversationKind.ChatOnModel
    conversation.scratch -> ConversationKind.ChatOnAgent
    conversation.status == ConversationStatus.Failed -> ConversationKind.WorkspaceFailed
    working -> ConversationKind.WorkspaceWorking
    else -> ConversationKind.WorkspaceIdle
}

fun barControls(kind: ConversationKind, harness: HarnessReport): List<BarControl> {
    val starting = kind == ConversationKind.NewConversation || kind == ConversationKind.NewChat
    val workspace = kind == ConversationKind.WorkspaceIdle || kind == ConversationKind.WorkspaceWorking || kind == ConversationKind.WorkspaceFailed
    val configurable = workspace || kind == ConversationKind.ChatOnAgent

    return listOfNotNull(
        BarControl.Attach,
        BarControl.Project.takeIf { kind == ConversationKind.NewConversation },
        BarControl.Place.takeIf { kind == ConversationKind.NewConversation },
        BarControl.Actions.takeIf { kind == ConversationKind.Assistant },
        BarControl.Dictate.takeIf { harness.voice && !starting },
        BarControl.TeachMe.takeIf { harness.teachMe && !starting },
        BarControl.Goal.takeIf { workspace },
        BarControl.Account,
        BarControl.Model.takeIf { harness.models && (configurable || kind == ConversationKind.NewConversation) },
        BarControl.Mode.takeIf { harness.modes && configurable },
        BarControl.Settings.takeIf { harness.settings && configurable },
        BarControl.Usage.takeIf { !starting },
        BarControl.Stop.takeIf { harness.working && !starting },
        BarControl.Send,
    )
}

fun List<BarControl>.inGroups(vararg groups: BarGroup): List<BarControl> = filter { it.group in groups }

@Composable
fun ComposerBar(controls: List<BarControl>, modifier: Modifier = Modifier, render: @Composable (BarControl) -> Unit) {
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        controls.inGroups(BarGroup.Tools).forEach { render(it) }

        Row(
            Modifier.weight(1f).horizontalScrollFaded(),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1, Alignment.End),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            controls.inGroups(BarGroup.Pickers).forEach { render(it) }
        }

        controls.inGroups(BarGroup.Usage, BarGroup.Turn).forEach { render(it) }
    }
}
