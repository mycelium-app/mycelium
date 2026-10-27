package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.PuzzleSummary
import mycelium.state.Puzzle
import mycelium.state.daysText
import mycelium.state.dayLabel
import mycelium.state.secondsText

@Composable
fun PuzzleBar(title: String, day: String?, status: String, onBack: () -> Unit) {
    val colors = MyTheme.colors

    Bar {
        MyButton(onBack, icon = MyIcon.ChevronLeft, description = "Back to puzzles", muted = true)
        Label(title, Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)
        Label(day?.let { dayLabel(it, "") }.orEmpty(), Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
        Label(status, Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta)
    }
}

@Composable
fun PuzzleStat(name: String, value: String, note: String? = null) {
    val colors = MyTheme.colors

    Column(
        Modifier.widthIn(min = 140.dp).border(Tokens.Size.Hairline, colors.border, ControlShape).padding(Tokens.Space.S3).semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
    ) {
        Label(name, style = Tokens.Type.Meta, color = colors.textMuted)
        Label(value, style = Tokens.Type.Title)

        if (note != null) {
            Label(note, style = Tokens.Type.Meta, color = colors.textMuted)
        }
    }
}

@Composable
fun PuzzleFinished(headline: String, seconds: Int?, summary: PuzzleSummary?, onBack: () -> Unit, solved: Boolean, detail: String? = null) {
    val colors = MyTheme.colors
    val stats = summary?.stats

    Column(Modifier.widthIn(max = 480.dp), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(if (solved) MyIcon.Check else MyIcon.Close, if (solved) colors.statusDone else colors.statusFailed, Tokens.Size.IconMd)
            Label(headline, Modifier.semantics { heading() }, Tokens.Type.Title)
        }

        if (detail != null) {
            Label(detail, color = colors.textMuted)
        }

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            if (seconds != null) {
                PuzzleStat("Time", secondsText(seconds))
            }

            if (stats != null) {
                PuzzleStat("Streak", daysText(stats.streak), "Best ${daysText(stats.bestStreak)}")
            }
        }

        MyButton(onBack, text = "Back to puzzles", variant = ButtonVariant.Primary)
    }
}

@Composable
fun PuzzleLoadFailed(puzzle: Puzzle, onRetry: () -> Unit, onPlayOnSite: () -> Unit, onBack: () -> Unit) {
    val colors = MyTheme.colors

    Column(Modifier.fillMaxSize()) {
        PuzzleBar(puzzle.name, null, "", onBack)

        EmptyState("Could not load today's ${puzzle.name}", Modifier.weight(1f).fillMaxWidth(), alert = true) {
            Label("The New York Times did not send the puzzle. Try again, or play it on ${puzzle.host}, where Mycelium reads your result from the page.", color = colors.textMuted)

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton(onRetry, text = "Try again", variant = ButtonVariant.Outline)
                MyButton(onPlayOnSite, text = "Play on ${puzzle.host}", variant = ButtonVariant.Primary)
            }
        }
    }
}
