package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import mycelium.api.Hours
import mycelium.api.HoursDay
import mycelium.state.Store
import mycelium.state.WeekdayNames
import mycelium.state.hoursSummary

private const val AlwaysOpen = "Always open"
private const val SetHours = "Set hours"
private val ValidTime = Regex("""(([01]\d|2[0-3]):[0-5]\d)|24:00""")

@Composable
fun HoursEditor(name: String, hours: Hours, onChange: (Hours) -> Unit) {
    val colors = MyTheme.colors
    val sorted = hours.days.sortedBy { it.weekday }

    fun change(day: HoursDay) {
        onChange(hours.copy(days = hours.days.map { if (it.weekday == day.weekday) day else it }))
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Segmented(listOf(AlwaysOpen, SetHours), if (hours.alwaysOpen) AlwaysOpen else SetHours, { picked -> onChange(hours.copy(alwaysOpen = picked == AlwaysOpen)) }, groupLabel = "$name hours")

        if (!hours.alwaysOpen) {
            Table(
                listOf(
                    Col("Day", 96.dp, weight = 1f),
                    Col("Opens", 88.dp, preferred = 96.dp),
                    Col("Closes", 88.dp, preferred = 96.dp),
                    Col("Closed all day", 72.dp, preferred = 104.dp),
                ),
                sorted.size,
                { row, column ->
                    val day = sorted[row]
                    val weekday = WeekdayNames[day.weekday]

                    when (column) {
                        0 -> TableCell.Text(weekday, color = if (day.closed) colors.textFaint else colors.text)
                        1 -> TableCell.Slot("00:00", 0.dp) { if (!day.closed) TimeField("$name opens on $weekday", day.opens) { change(day.copy(opens = it)) } }
                        2 -> TableCell.Slot("00:00", 0.dp) { if (!day.closed) TimeField("$name closes on $weekday", day.closes) { change(day.copy(closes = it)) } }
                        else -> TableCell.Slot("Closed all day", 0.dp) { Checkbox(day.closed, "$weekday closed all day", { change(day.copy(closed = !day.closed)) }, checkedState = "On", uncheckedState = "Off") }
                    }
                },
                rowKey = { sorted[it].weekday },
                rowDescription = { WeekdayNames[sorted[it].weekday] },
            )
        }
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
        error = "Enter a time like 09:00".takeIf { invalid },
        modifier = Modifier.widthIn(max = 96.dp),
    )
}

@Composable
fun GroupHoursTable(store: Store, onOpenGroup: (String) -> Unit) {
    val state by store.state.collectAsState()
    val groups = state.groups.values.sortedBy { it.name.lowercase() }
    val muted = MyTheme.colors.textMuted
    val faint = MyTheme.colors.textFaint

    LaunchedEffect(Unit) {
        store.load { store.refreshGroups() }
    }

    if (groups.isEmpty()) {
        return
    }

    Table(
        listOf(
            Col("Group", 120.dp, weight = 1f),
            Col("Hours", 140.dp, preferred = 220.dp),
            ActionColumn,
        ),
        groups.size,
        { row, column ->
            val group = groups[row]

            when (column) {
                0 -> TableCell.Slot(group.name) {
                    Label(group.name, Modifier.pressable({ onOpenGroup(group.id) }, role = Role.Button, description = "Open the ${group.name} group hours", shape = BlockShape).padding(vertical = Tokens.Space.S1), maxLines = 1)
                }

                1 -> TableCell.Text(hoursSummary(group.hours), color = muted)
                else -> TableCell.Slot { Icon(MyIcon.ChevronRight, faint, Tokens.Size.IconSm) }
            }
        },
        rowKey = { groups[it].id },
        rowDescription = { groups[it].name },
    )
}
