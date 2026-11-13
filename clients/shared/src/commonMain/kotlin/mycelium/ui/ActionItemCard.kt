package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlin.time.Clock
import kotlinx.coroutines.delay
import mycelium.api.ActionItem
import mycelium.state.ActionItemPhase
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.cleanText
import mycelium.state.cleanTerminalText
import mycelium.state.elapsedSeconds
import mycelium.state.formatElapsed
import mycelium.state.parsedMarkdown
import mycelium.state.phase
import mycelium.state.spokenElapsed

private const val ElapsedTickMillis = 1_000L

private val OutputMaxHeight = 164.dp

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ActionItemCard(store: Store, state: AppState, conversationId: String, request: InboxRequest.ActionItem, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val item = request.item
    val command = item.command
    val run = item.run
    val failure = state.actionItemFailures[item.id]
    val phase = when {
        request.done -> ActionItemPhase.Done
        request.succeeded -> ActionItemPhase.Succeeded
        failure != null && run == null -> ActionItemPhase.Failed
        else -> item.phase(state.terminalExitCodes)
    }
    val conversation = state.conversations[conversationId]
    val node = item.nodeName ?: "its node"

    fun done() = store.completeActionItem(state.conversations.getValue(conversationId), item.id)

    Card {
        CardHead {
            when (phase) {
                ActionItemPhase.Waiting -> StatusMark("For you to do", Tone.Attention, MyIcon.Clipboard)
                ActionItemPhase.Running -> StatusMark("Running", Tone.Working, MyIcon.Working)
                ActionItemPhase.Succeeded -> StatusMark("Succeeded", Tone.Done, MyIcon.CircleCheck)
                ActionItemPhase.Failed -> StatusMark("Failed", Tone.Failed, MyIcon.Failed)
                ActionItemPhase.Stopped -> StatusMark("Stopped", Tone.Muted, MyIcon.Stop)
                ActionItemPhase.Done -> StatusMark("Done", Tone.Muted, MyIcon.Check)
            }

            if (command == null && !request.done) {
                Spacer(Modifier.weight(1f))
                Icon(MyIcon.Clock, colors.textMuted, Tokens.Size.IconSm)
                Label("Stays until you mark it done", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            } else {
                if (phase != ActionItemPhase.Failed) {
                    Spacer(Modifier.weight(1f))
                }

                when (phase) {
                    ActionItemPhase.Waiting -> {
                        Icon(MyIcon.Clock, colors.textMuted, Tokens.Size.IconSm)
                        Label("Resolves when the command succeeds", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                    }
                    ActionItemPhase.Running -> RunningTimer(run!!.elapsedSeconds(Clock.System.now()), run.startedAt)
                    ActionItemPhase.Succeeded -> Label("Exit code 0", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                    ActionItemPhase.Failed -> Label(run?.exitCode?.let { "Exit code $it" } ?: run?.reason ?: failure.orEmpty(), Modifier.weight(1f), Tokens.Type.Meta.copy(textAlign = TextAlign.End), colors.textMuted, maxLines = 1)
                    ActionItemPhase.Stopped, ActionItemPhase.Done -> Unit
                }
            }
        }

        Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            if (phase == ActionItemPhase.Done && command != null) {
                Label(item.text, style = Tokens.Type.Ui, color = colors.textMuted)
                CommandBox(command, copyable = false)
            } else {
                MarkdownBlocks(parsedMarkdown(item.text), textStyle = Tokens.Type.Ui, blockSpacing = Tokens.Space.S2)

                if (command != null) {
                    CommandBox(command, copyable = true)
                    PlaceRow(state, item)

                    if (phase != ActionItemPhase.Waiting) {
                        val reason = run?.reason ?: failure
                        val feed = run?.let { state.terminalOutput[it.terminalId] }
                        val streamed = feed?.cleanText().orEmpty()
                        val recorded = run?.output?.let { cleanTerminalText(it) }.orEmpty()
                        val output = listOf(if (phase == ActionItemPhase.Running) streamed.ifBlank { recorded } else recorded.ifBlank { streamed }, reason.orEmpty()).filter { it.isNotBlank() }.joinToString("\n")

                        OutputArea(command, output)
                    }
                }
            }

            if (request.done) {
                Label("Marked as done. It leaves Needs you.", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
            } else if (phase == ActionItemPhase.Succeeded) {
                Label("It leaves Needs you.", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
            } else if (command == null) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                    MyButton(::done, text = "Mark as done", variant = ButtonVariant.Primary, description = "Mark as done: ${item.text}")
                }
            }
        }

        if (command != null && !request.done && phase != ActionItemPhase.Succeeded) {
            FlowRow(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                val openInTerminal = {
                    if (run != null) {
                        store.openActionItemTerminal(conversationId, run.terminalId) { onOpenConversation(conversationId) }
                    }
                }

                when (phase) {
                    ActionItemPhase.Waiting -> {
                        MyButton({ conversation?.let { store.runActionItem(it, item.id) } }, text = "Run", icon = MyIcon.Play, variant = ButtonVariant.Primary, description = "Run $command on $node")
                        MyButton(::done, text = "Mark as done", variant = ButtonVariant.Outline, description = "Mark as done: ${item.text}")
                    }
                    ActionItemPhase.Running -> {
                        MyButton({ store.stopActionItemRun(run!!.terminalId) }, text = "Stop", icon = MyIcon.Stop, variant = ButtonVariant.Outline, description = "Stop $command")
                        MyButton(openInTerminal, text = "Open in terminal", icon = MyIcon.Terminal, variant = ButtonVariant.Outline, description = "Open in terminal: $command")
                    }
                    ActionItemPhase.Failed, ActionItemPhase.Stopped -> {
                        MyButton({ conversation?.let { store.runActionItem(it, item.id) } }, text = "Retry", icon = MyIcon.Refresh, variant = ButtonVariant.Primary, description = "Retry $command on $node")
                        MyButton(::done, text = "Mark as done", variant = ButtonVariant.Outline, description = "Mark as done: ${item.text}")

                        if (run != null) {
                            MyButton(openInTerminal, text = "Open in terminal", icon = MyIcon.Terminal, variant = ButtonVariant.Outline, description = "Open in terminal: $command")
                        }
                    }
                    ActionItemPhase.Succeeded, ActionItemPhase.Done -> Unit
                }
            }
        }
    }
}

@Composable
private fun RunningTimer(seconds: Long, key: String) {
    val colors = MyTheme.colors
    var shown by remember(key) { mutableStateOf(seconds) }

    LaunchedEffect(key) {
        while (true) {
            delay(ElapsedTickMillis)
            shown += 1
        }
    }

    Label(formatElapsed(shown), Modifier.semantics { contentDescription = "Running for ${spokenElapsed(shown)}" }, Tokens.Type.Meta, colors.textMuted, maxLines = 1)
}

@Composable
private fun CommandBox(command: String, copyable: Boolean) {
    val colors = MyTheme.colors
    val clipboard = LocalClipboardManager.current

    Row(
        Modifier.fillMaxWidth().background(colors.bgPanel, ControlShape).border(Tokens.Size.Hairline, colors.border, ControlShape).padding(start = Tokens.Space.S3, top = Tokens.Space.S1, end = Tokens.Space.S1, bottom = Tokens.Space.S1),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(command, Modifier.weight(1f), Tokens.Type.Code.themed().copy(color = if (copyable) colors.text else colors.textMuted))

        if (copyable) {
            MyButton({ clipboard.setText(AnnotatedString(command)) }, text = "Copy", icon = MyIcon.Copy, description = "Copy the command", size = ButtonSize.Sm, muted = true)
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun PlaceRow(state: AppState, item: ActionItem) {
    val colors = MyTheme.colors
    val node = item.nodeId?.let { state.nodes[it] }

    FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        item.nodeName?.let { name ->
            Row(Modifier.semantics(mergeDescendants = true) { contentDescription = "Node $name" }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                Icon(MyIcon.Server, node?.let { nodeColour(it) } ?: colors.textMuted, Tokens.Size.IconSm)
                Label(name, style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        item.folder?.let { folder ->
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                Icon(MyIcon.Folder, colors.textMuted, Tokens.Size.IconSm)
                Label(folder, style = Tokens.Type.Code.copy(fontSize = Tokens.Type.Meta.fontSize, lineHeight = Tokens.Type.Meta.lineHeight), color = colors.textMuted)
            }
        }
    }
}

@Composable
private fun OutputArea(command: String, output: String) {
    val colors = MyTheme.colors
    val scroll = rememberScrollState()

    LaunchedEffect(output) {
        scroll.scrollTo(scroll.maxValue)
    }

    Box(
        Modifier
            .fillMaxWidth()
            .heightIn(max = OutputMaxHeight)
            .background(colors.bgPanel, ControlShape)
            .border(Tokens.Size.Hairline, colors.border, ControlShape)
            .semantics { contentDescription = "Output of $command"; liveRegion = LiveRegionMode.Polite }
            .verticalScroll(scroll)
            .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5),
    ) {
        BasicText(output, style = Tokens.Type.Code.themed().copy(color = colors.text))
    }
}
