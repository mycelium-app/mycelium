package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.state.SnoozeChoice
import mycelium.state.clockTime
import mycelium.state.customSnoozeError
import mycelium.state.customSnoozeUntil
import mycelium.state.snoozeLeft
import mycelium.state.snoozeUntil
import kotlin.time.Clock

const val SnoozedName = "Snoozed"

@Composable
fun SnoozeMenu(onSnooze: (String) -> Unit, onDismiss: () -> Unit) {
    val platform = LocalPlatform.current
    val localDateTime = { millis: Long -> platform?.localDateTime(millis) }
    val now = remember { Clock.System.now() }
    var custom by remember { mutableStateOf(false) }
    var text by remember { mutableStateOf("") }
    var failure by remember { mutableStateOf<String?>(null) }

    if (custom) {
        val save = {
            val moment = Clock.System.now()
            val until = customSnoozeUntil(text, moment, localDateTime)

            if (until == null) {
                failure = customSnoozeError(text, moment, localDateTime)
            } else {
                onSnooze(until.toString())
                onDismiss()
            }
        }

        RenameDialog("Snooze until", "Date and time", text, { text = it; failure = null }, save, onDismiss, failure, onDismissFailure = { failure = null })

        return
    }

    MenuPopup(onDismiss = onDismiss) {
        MenuLabel("Snooze until")

        for (choice in SnoozeChoice.entries) {
            val until = snoozeUntil(choice, now, localDateTime)

            MenuItem(choice.label, onClick = { onSnooze(until.toString()); onDismiss() }, hint = clockTime(until.toString(), now, localDateTime), leading = MyIcon.Clock)
        }

        MenuItem("Custom", onClick = { custom = true }, leading = MyIcon.Pencil)
    }
}

@Composable
fun SnoozedLine(until: String, onWake: () -> Unit, modifier: Modifier = Modifier) {
    val left = snoozeLeft(until, Clock.System.now())

    Row(modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        Label("Snoozed, $left", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        MyButton(onWake, text = "Wake now", description = "Wake this conversation now", size = ButtonSize.Sm, muted = true)
    }
}
