package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.PuzzleDay
import mycelium.state.CalendarCell
import mycelium.state.CalendarMonth
import mycelium.state.Puzzle
import mycelium.state.calendarDescription
import mycelium.state.canStepBack
import mycelium.state.canStepForward
import mycelium.state.cells
import mycelium.state.monthOf
import mycelium.state.shortScore

private val WeekdayInitials = listOf("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")
private val CalendarMaxWidth = 388.dp
private val CalendarCellHeight = 48.dp
private val CalendarGap = 4.dp
private val CalendarIconSize = 12.dp

@Composable
fun PuzzleCalendar(puzzle: Puzzle, history: List<PuzzleDay>, today: String, modifier: Modifier = Modifier) {
    var month by remember(puzzle.id, today) { mutableStateOf(monthOf(today)) }
    val colors = MyTheme.colors
    val canStepBack = month.canStepBack(history)
    val canStepForward = month.canStepForward(today)
    val grid = month.cells(today, history)
    val rows = grid.chunked(WeekdayInitials.size)

    Column(modifier.widthIn(max = CalendarMaxWidth), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
            Label("History", Modifier.weight(1f).semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)
            MyButton({ if (canStepBack) month = month.shifted(-1) }, Modifier.semantics { if (!canStepBack) disabled() }, icon = MyIcon.ChevronLeft, description = "Previous month", size = ButtonSize.Sm, muted = true)
            Label(month.title, style = Tokens.Type.UiStrong)
            MyButton({ if (canStepForward) month = month.shifted(1) }, Modifier.semantics { if (!canStepForward) disabled() }, icon = MyIcon.ChevronRight, description = "Next month", size = ButtonSize.Sm, muted = true)
        }

        Column(Modifier.semantics { contentDescription = month.title }, verticalArrangement = Arrangement.spacedBy(CalendarGap)) {
            Row(Modifier.clearAndSetSemantics { }, horizontalArrangement = Arrangement.spacedBy(CalendarGap)) {
                for (name in WeekdayInitials) {
                    Label(name, Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                }
            }

            for (row in rows) {
                Row(horizontalArrangement = Arrangement.spacedBy(CalendarGap)) {
                    for (index in 0 until WeekdayInitials.size) {
                        val cell = row.getOrNull(index)

                        if (cell == null) {
                            Spacer(Modifier.weight(1f).height(CalendarCellHeight))
                        } else {
                            CalendarDay(puzzle, cell, Modifier.weight(1f))
                        }
                    }
                }
            }
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
            Icon(MyIcon.Pencil, colors.textMuted, CalendarIconSize)
            Label("Entered by hand", style = Tokens.Type.Meta, color = colors.textMuted)
        }
    }
}

@Composable
private fun CalendarDay(puzzle: Puzzle, cell: CalendarCell, modifier: Modifier) {
    val colors = MyTheme.colors
    val entry = cell.entry?.result
    val outline = when {
        cell.today -> Modifier.border(Tokens.Size.FocusRing, colors.accent, ControlShape)
        cell.future -> Modifier
        else -> Modifier.border(Tokens.Size.Hairline, colors.border, ControlShape)
    }
    val described = if (cell.future) Modifier.clearAndSetSemantics { } else Modifier.clearAndSetSemantics { contentDescription = puzzle.calendarDescription(cell) }

    Column(
        modifier
            .height(CalendarCellHeight)
            .then(if (cell.future) Modifier.alpha(0.5f) else Modifier)
            .then(outline)
            .then(described)
            .padding(horizontal = Tokens.Space.S1, vertical = Tokens.Space.S0_5),
        verticalArrangement = Arrangement.SpaceBetween,
    ) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            Label(cell.day.toString(), style = Tokens.Type.Meta, color = colors.textMuted)

            if (entry?.source == "manual") {
                Icon(MyIcon.Pencil, colors.textMuted, CalendarIconSize)
            }
        }

        if (entry != null) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                val failed = entry.solved == false

                Icon(if (failed) MyIcon.Close else MyIcon.Check, if (failed) colors.statusFailed else colors.statusDone, CalendarIconSize)
                Label(puzzle.shortScore(entry), style = Tokens.Type.UiStrong)
            }
        }
    }
}
