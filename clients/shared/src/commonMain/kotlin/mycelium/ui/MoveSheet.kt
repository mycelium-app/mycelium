package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.launch
import mycelium.api.ConversationStatus
import mycelium.api.Conversation
import mycelium.api.MovePlan
import mycelium.api.MoveRequest
import mycelium.api.MoveState
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.goesWithText
import mycelium.state.staysHereText

@Composable
fun MoveSheet(store: Store, state: AppState, conversation: Conversation, onDismiss: () -> Unit, onOpenConversation: (String) -> Unit, preferredNodeId: String? = null) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    val targets = state.nodes.values.filter { it.id != conversation.nodeId && it.online && !it.draining }.sortedBy { it.name }
    var chosenId by remember { mutableStateOf<String?>(null) }
    var requested by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    val target = targets.firstOrNull { it.id == chosenId } ?: targets.firstOrNull { it.id == preferredNodeId } ?: targets.firstOrNull()
    var plan by remember(target?.id) { mutableStateOf<MovePlan?>(null) }
    val progress = state.moves[conversation.id]?.takeIf { it.state == MoveState.Waiting || it.state == MoveState.Moving || requested }
    val busy = progress?.state == MoveState.Waiting || progress?.state == MoveState.Moving
    val summary = plan?.takeIf { it.movable }?.let { "${goesWithText(it)} ${staysHereText(it)}" }
    val problem = failure ?: progress?.message.takeIf { progress?.state == MoveState.Failed } ?: plan?.reason.takeIf { plan?.movable == false }

    LaunchedEffect(target?.id) {
        val chosen = target ?: return@LaunchedEffect
        failure = attempt { plan = store.api.movePlan(conversation.id, chosen.id) }
    }

    LaunchedEffect(progress?.state) {
        if (requested && progress != null && progress.state == MoveState.Done) {
            val forked = progress.resultId.takeIf { progress.fork }
            onDismiss()
            forked?.let(onOpenConversation)
        }
    }

    fun submit(fork: Boolean) {
        val chosen = target

        if (chosen == null) {
            failure = "Choose the node to move to. Start or join another node if none is listed."
            return
        }

        val held = plan

        if (held != null && !held.movable) {
            failure = held.reason
            return
        }

        failure = null
        requested = true

        scope.launch {
            failure = attempt { store.requestMove(conversation.id, MoveRequest(chosen.id, fork)) }

            if (failure != null) {
                requested = false
            }
        }
    }

    MenuPopup(onDismiss = onDismiss, width = Tokens.Size.LoginPopover) {
        Column(Modifier.padding(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Move to", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            if (targets.isEmpty()) {
                Label("No other node is online. Start or join another node, then move again.", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                for (option in targets) {
                    MyButton(
                        { chosenId = option.id },
                        text = option.name,
                        icon = nodeIcon(option),
                        description = "Move to ${option.name}",
                        variant = ButtonVariant.Outline,
                        size = ButtonSize.Sm,
                        muted = option.id != target?.id,
                        selected = option.id == target?.id,
                    )
                }
            }

            summary?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted) }

            if (progress != null) {
                when (progress.state) {
                    MoveState.Waiting -> Label("Waiting for the turn to end. The move starts as soon as it does.", style = Tokens.Type.Meta, color = colors.textMuted)
                    MoveState.Moving -> IndeterminateProgress("Moving to ${state.nodes[progress.nodeId]?.name.orEmpty()}")
                    else -> Unit
                }
            }

            problem?.let { ErrorBanner(it, onDismiss = { failure = null; requested = false }) }

            FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                if (progress?.state == MoveState.Waiting) {
                    MyButton({ scope.launch { failure = attempt { store.cancelMove(conversation.id) } } }, text = "Cancel move", size = ButtonSize.Sm, muted = true)
                } else if (!busy) {
                    MyButton({ submit(true) }, text = "Fork instead", size = ButtonSize.Sm)
                    MyButton({ submit(false) }, text = if (conversation.status == ConversationStatus.Working || conversation.status == ConversationStatus.Input) "Move after this turn" else "Move", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                }
            }
        }
    }
}
