package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.time.Clock
import mycelium.api.WellnessBreak
import mycelium.state.Store
import mycelium.state.WindDownMinutes
import mycelium.state.breakWhen
import mycelium.state.instantOrNull
import mycelium.state.lengthText
import mycelium.state.localTimeOf

private val ReminderChoices = listOf(25, 30, 45, 50, 60, 90)
private val SnoozeChoices = listOf(5, 10, 15)
private val BreakChoices = listOf(5, 10, 15)

private fun minutesName(minutes: Int) = "$minutes min"

@Composable
fun TimeAndFocusSettings(store: Store, onOpenGroup: (String) -> Unit = {}) {
    val colors = MyTheme.colors
    val wellness by store.wellness.state.collectAsState()
    val settings = wellness.settings
    val platform = LocalPlatform.current
    val platformZone = platform?.timeZone()
    val recentBreaks = wellness.overview?.recentBreaks.orEmpty()
    val today = localTimeOf(platform, Clock.System.now()).substringBefore('T')
    val whenOf = { past: WellnessBreak -> breakWhen(localTimeOf(platform, instantOrNull(past.startedAt) ?: Clock.System.now()), today) }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        Label("Time and focus", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Label("Breaks", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            SettingSwitch("Remind me to take a break", settings.breakReminders, true) { on -> store.wellness.update { it.copy(breakReminders = on, timeZone = it.timeZone ?: platformZone) } }
            Choice("Remind after", ReminderChoices, settings.breakAfterMinutes) { choice -> store.wellness.update { it.copy(breakAfterMinutes = choice) } }
            Choice("Snooze for", SnoozeChoices, settings.snoozeMinutes) { choice -> store.wellness.update { it.copy(snoozeMinutes = choice) } }
            Choice("Break length", BreakChoices, settings.breakMinutes) { choice -> store.wellness.update { it.copy(breakMinutes = choice) } }

            if (recentBreaks.isNotEmpty()) {
                Label("Recent breaks", Modifier.semantics { heading() }, Tokens.Type.Meta, colors.textMuted)
                Table(
                    listOf(Col("When", 140.dp, preferred = 140.dp), Col("Length", 150.dp, preferred = 150.dp), Col("What you did", 160.dp, weight = 1f, overflow = Overflow.Wrap)),
                    recentBreaks.size,
                    { row, column ->
                        when (column) {
                            0 -> TableCell.Text(whenOf(recentBreaks[row]), color = colors.textMuted)
                            1 -> TableCell.Text(recentBreaks[row].lengthText(), color = colors.textMuted)
                            else -> TableCell.Text(recentBreaks[row].note.orEmpty())
                        }
                    },
                    rowKey = { recentBreaks[it].id },
                    rowDescription = { "Break ${whenOf(recentBreaks[it])}" },
                )
                Label("The Wellness assistant reads these when it drafts your weekly reflection.", style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Label("Hours", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Label("Outside its hours a group leaves the sidebar, every list and the group selector, and its notifications wait. Agents keep working. $WindDownMinutes minutes before it closes, a countdown says so.", style = Tokens.Type.Meta, color = colors.textMuted)
            Label("Mycelium", style = Tokens.Type.Meta, color = colors.textMuted)
            HoursEditor("Mycelium", settings.hours) { hours -> store.wellness.update { it.copy(hours = hours, timeZone = it.timeZone ?: platformZone) } }
            Label("Every group is closed while Mycelium is closed.", style = Tokens.Type.Meta, color = colors.textMuted)
            Label("Groups", style = Tokens.Type.Meta, color = colors.textMuted)
            GroupHoursTable(store, onOpenGroup)
            Label("New groups are always open.", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("While closed", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            SettingSwitch("Hold finished-agent notifications", settings.quietHoldNotifications, true) { on -> store.wellness.update { it.copy(quietHoldNotifications = on) } }
            SettingSwitch("Hold scheduled runs", settings.quietHoldSchedules, true) { on -> store.wellness.update { it.copy(quietHoldSchedules = on) } }
            SettingSwitch("Hold assistant pings", settings.quietHoldPings, true) { on -> store.wellness.update { it.copy(quietHoldPings = on) } }
            Label("An assistant that messages you while its group is closed keeps working; its notification and sidebar badge wait until the group opens.", style = Tokens.Type.Meta, color = colors.textMuted)
        }
    }
}

@Composable
private fun SettingSwitch(label: String, on: Boolean, enabled: Boolean, onChange: (Boolean) -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Checkbox(on, label, { if (enabled) onChange(!on) }, checkedState = "On", uncheckedState = "Off")
        Label(label, color = if (enabled) MyTheme.colors.text else MyTheme.colors.textFaint)
    }
}

@Composable
private fun Choice(title: String, choices: List<Int>, chosen: Int, onChoose: (Int) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
        Label(title, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        Segmented(choices.map(::minutesName), minutesName(chosen), { picked -> onChoose(choices.first { minutesName(it) == picked }) }, groupLabel = title)
    }
}
