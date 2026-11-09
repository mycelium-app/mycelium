package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import mycelium.api.ConversationStatus
import mycelium.api.Watch
import mycelium.state.clockTime

fun watchTitle(watch: Watch): String = when (watch.kind) {
    "timer" -> watch.note.orEmpty()
    "release" -> "The ${watch.reference.orEmpty()} release of ${watch.repository.orEmpty()}"
    "pipeline" -> "The ${watch.reference.orEmpty()} run of ${watch.repository.orEmpty()}"
    else -> watch.title.orEmpty()
}

fun watchState(watch: Watch, status: ConversationStatus?, localDateTime: (Long) -> String?): String = when (watch.kind) {
    "timer" -> "Wake-up · ${clockTime(watch.dueAt.orEmpty(), localDateTime = localDateTime)}"
    "release" -> "Release · Running"
    "pipeline" -> "Pipeline · Running"
    else -> when (status) {
        ConversationStatus.Working -> "Conversation · Working"
        ConversationStatus.Input, ConversationStatus.Review -> "Conversation · Waiting on you"
        ConversationStatus.Done -> "Conversation · Finished"
        ConversationStatus.Failed -> "Conversation · Failed"
        null -> "Conversation"
    }
}

@Composable
fun WatchingButton(watches: List<Watch>, statusOf: (String) -> ConversationStatus?, onOpen: (String) -> Unit, onStop: (Watch) -> Unit, compact: Boolean = false) {
    if (watches.isEmpty()) {
        return
    }

    var open by remember { mutableStateOf(false) }
    val label = "Watching ${watches.size}"

    Box {
        if (compact) {
            MyButton({ open = true }, icon = MyIcon.Eye, description = label, muted = true, selected = open)
        } else {
            MyButton({ open = true }, text = label, icon = MyIcon.Eye, trailingIcon = MyIcon.ChevronDown, description = "$label, show the list", size = ButtonSize.Sm, muted = true, selected = open)
        }

        if (open) {
            WatchingMenu(watches, statusOf, onOpen, onStop, onDismiss = { open = false })
        }
    }
}

@Composable
fun WatchingMenu(watches: List<Watch>, statusOf: (String) -> ConversationStatus?, onOpen: (String) -> Unit, onStop: (Watch) -> Unit, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val localDateTime: (Long) -> String? = { millis -> platform?.localDateTime(millis) }

    MenuPopup(onDismiss = onDismiss, width = Tokens.Size.Popover) {
        Column(Modifier.semantics { contentDescription = "Watching" }) {
            for (watch in watches) {
                val title = watchTitle(watch)
                val status = watch.conversationId?.let(statusOf)
                val state = watchState(watch, status, localDateTime)
                val target = watch.conversationId.takeIf { watch.kind == "conversation" }
                val timer = watch.kind == "timer"
                val (icon, tint) = when {
                    timer -> MyIcon.Clock to colors.textMuted
                    target == null -> MyIcon.Working to Tone.Working.color()
                    status == null -> MyIcon.Eye to colors.textMuted
                    else -> iconOf(status) to toneOf(status).color()
                }

                Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(start = Tokens.Space.S2, end = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    val opens = if (target != null) Modifier.pressable({ onDismiss(); onOpen(target) }, role = Role.Button, description = "Open $title, ${state.substringAfter(" · ").lowercase()}", shape = ControlShape) else Modifier

                    Row(Modifier.weight(1f).then(opens).padding(vertical = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                        Icon(icon, tint, Tokens.Size.IconSm)
                        Column {
                            Label(title, maxLines = 1)
                            Label(state, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                        }
                    }

                    MyButton({ onStop(watch) }, icon = MyIcon.Close, description = if (timer) "Cancel the wake-up: $title" else "Stop watching ${title.replaceFirstChar { it.lowercase() }}", size = ButtonSize.Sm, muted = true)
                }
            }
        }
    }
}

@Composable
fun WaitingForAnswerLine(modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        Icon(MyIcon.Alert, MyTheme.colors.statusAttention, Tokens.Size.IconSm)
        Label("Waiting for your answer", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
    }
}
