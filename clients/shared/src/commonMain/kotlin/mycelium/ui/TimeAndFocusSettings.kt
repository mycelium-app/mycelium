package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.QuietDay
import mycelium.state.Store
import mycelium.state.WeekdayNames

private val ReminderChoices = listOf(25, 30, 45, 50, 60, 90)
private val SnoozeChoices = listOf(5, 10, 15)
private val BreakChoices = listOf(5, 10, 15)
private val ValidTime = Regex("""([01]\d|2[0-3]):[0-5]\d""")

private fun minutesName(minutes: Int) = "$minutes min"

@Composable
fun TimeAndFocusSettings(store: Store) {
    val colors = MyTheme.colors
    val wellness by store.wellness.state.collectAsState()
    val settings = wellness.settings
    val platformZone = LocalPlatform.current?.timeZone()

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        Label("Time and focus", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Label("Breaks", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            SettingSwitch("Remind me to take a break", settings.breakReminders && !settings.pomodoro, !settings.pomodoro) { on -> store.wellness.update { it.copy(breakReminders = on, timeZone = it.timeZone ?: platformZone) } }
            Choice("Remind after", ReminderChoices, settings.breakAfterMinutes) { choice -> store.wellness.update { it.copy(breakAfterMinutes = choice) } }
            Choice("Snooze for", SnoozeChoices, settings.snoozeMinutes) { choice -> store.wellness.update { it.copy(snoozeMinutes = choice) } }
            Choice("Break length", BreakChoices, settings.breakMinutes) { choice -> store.wellness.update { it.copy(breakMinutes = choice) } }
            SettingSwitch("Pomodoro", settings.pomodoro, true) { on -> store.wellness.update { it.copy(pomodoro = on) } }
            Label("Pomodoro works in 25 minute blocks with 5 minute breaks, and 15 minutes after the fourth. It replaces break reminders while it is on.", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Label("Quiet hours", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            SettingSwitch("Hold notifications during quiet hours", settings.quietHours, true) { on -> store.wellness.update { it.copy(quietHours = on, timeZone = it.timeZone ?: platformZone) } }

            if (settings.quietHours) {
                QuietDays(settings.quietDays) { day -> store.wellness.update { current -> current.copy(quietDays = current.quietDays.map { if (it.weekday == day.weekday) day else it }) } }

                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    SettingSwitch("Hold finished-agent notifications", settings.quietHoldNotifications, true) { on -> store.wellness.update { it.copy(quietHoldNotifications = on) } }
                    SettingSwitch("Hold scheduled runs", settings.quietHoldSchedules, true) { on -> store.wellness.update { it.copy(quietHoldSchedules = on) } }
                    SettingSwitch("Hold assistant pings", settings.quietHoldPings, true) { on -> store.wellness.update { it.copy(quietHoldPings = on) } }
                }
            }
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

@Composable
private fun QuietDays(days: List<QuietDay>, onChange: (QuietDay) -> Unit) {
    val sorted = days.sortedBy { it.weekday }
    val colors = MyTheme.colors

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            for (day in sorted) {
                MyButton({ onChange(day.copy(enabled = !day.enabled)) }, text = WeekdayNames[day.weekday].take(3), description = "${WeekdayNames[day.weekday]} quiet hours", variant = ButtonVariant.Outline, selected = day.enabled)
            }
        }

        Table(
            listOf(
                Col("Day", 96.dp, weight = 1f),
                Col("From", 88.dp, preferred = 96.dp),
                Col("Until", 88.dp, preferred = 96.dp),
                Col("All day", 72.dp, preferred = 72.dp),
            ),
            sorted.size,
            { row, column ->
                val day = sorted[row]

                when (column) {
                    0 -> TableCell.Text(WeekdayNames[day.weekday], color = if (day.enabled) colors.text else colors.textFaint)
                    1 -> TableCell.Slot("00:00", 0.dp) { if (day.enabled && !day.allDay) TimeField("${WeekdayNames[day.weekday]} quiet from", day.starts) { onChange(day.copy(starts = it)) } }
                    2 -> TableCell.Slot("00:00", 0.dp) { if (day.enabled && !day.allDay) TimeField("${WeekdayNames[day.weekday]} quiet until", day.ends) { onChange(day.copy(ends = it)) } }
                    else -> TableCell.Slot("All day", 0.dp) { if (day.enabled) Checkbox(day.allDay, "${WeekdayNames[day.weekday]} quiet all day", { onChange(day.copy(allDay = !day.allDay)) }, checkedState = "On", uncheckedState = "Off") }
                }
            },
            rowKey = { sorted[it].weekday },
            rowDescription = { WeekdayNames[sorted[it].weekday] },
        )
    }
}

@Composable
private fun TimeField(label: String, saved: String, onValid: (String) -> Unit) {
    var text by remember(saved) { mutableStateOf(saved) }
    val invalid = !ValidTime.matches(text)

    TextInput(
        text,
        { value ->
            text = value

            if (ValidTime.matches(value)) {
                onValid(value)
            }
        },
        label,
        showLabel = false,
        mono = true,
        error = "Enter a time like 22:00".takeIf { invalid },
        modifier = Modifier.widthIn(max = 96.dp),
    )
}
