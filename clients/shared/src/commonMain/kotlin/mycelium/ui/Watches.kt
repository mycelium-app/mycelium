package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import mycelium.api.Watch
import mycelium.state.clockTime

fun watchLine(watch: Watch, localDateTime: (Long) -> String?): String = when (watch.kind) {
    "timer" -> "Wakes ${clockTime(watch.dueAt.orEmpty(), localDateTime = localDateTime)}: ${watch.note.orEmpty()}"
    "release" -> "Watching the ${watch.reference.orEmpty()} release of ${watch.repository.orEmpty()}"
    "pipeline" -> "Watching the ${watch.reference.orEmpty()} run of ${watch.repository.orEmpty()}"
    else -> "Watching ${watch.title.orEmpty()}"
}

@Composable
fun WatchLines(watches: List<Watch>, onOpen: (String) -> Unit, onStop: (Watch) -> Unit, modifier: Modifier = Modifier) {
    if (watches.isEmpty()) {
        return
    }

    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val localDateTime: (Long) -> String? = { millis -> platform?.localDateTime(millis) }

    Column(modifier.fillMaxWidth().semantics { contentDescription = "Waiting for" }) {
        for (watch in watches) {
            val text = watchLine(watch, localDateTime)
            val timer = watch.kind == "timer"
            val target = watch.conversationId.takeIf { watch.kind == "conversation" }

            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Icon(if (timer) MyIcon.Clock else MyIcon.Eye, colors.textMuted, Tokens.Size.IconSm)

                if (target != null) {
                    Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                        Label("Watching", style = Tokens.Type.Meta, color = colors.textMuted)
                        Label(watch.title.orEmpty(), Modifier.pressable({ onOpen(target) }, role = Role.Button, description = "Open ${watch.title.orEmpty()}", shape = ControlShape), Tokens.Type.Meta, colors.text, maxLines = 1)
                    }
                } else {
                    Label(text, Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                }

                MyButton({ onStop(watch) }, text = if (timer) "Cancel" else "Stop", description = if (timer) "Cancel the wake-up: ${watch.note.orEmpty()}" else "Stop ${text.replaceFirstChar { it.lowercase() }}", size = ButtonSize.Sm, muted = true)
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
