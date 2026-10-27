package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.time.Clock
import kotlin.time.Instant
import mycelium.Platform
import mycelium.PageReading
import mycelium.api.PuzzleResult
import mycelium.api.PuzzleSummary
import mycelium.state.Puzzle
import mycelium.state.Puzzles
import mycelium.state.Store
import mycelium.state.dayLabel
import mycelium.state.isOnThisSite
import mycelium.state.parseGameState
import mycelium.state.puzzleStatus
import mycelium.state.readingScript
import mycelium.state.sourceLabel

private const val UnreadableGame = "Could not read the game"
private val PuzzleListWidth = 520.dp

private fun today(): String = Clock.System.now().toString().take(10)

@Composable
fun PuzzlesTab(store: Store, platform: Platform, onPlay: (Puzzle) -> Unit) {
    val wellness by store.wellness.state.collectAsState()
    var chosen by remember { mutableStateOf(Puzzles.first().id) }
    val selected = Puzzles.first { it.id == chosen }
    val summaries = wellness.puzzles.associateBy { it.puzzle }

    LaunchedEffect(Unit) {
        store.wellness.loadPuzzles()
    }

    val reads = platform.readsPages
    val colors = MyTheme.colors

    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val wide = reads && maxWidth >= 900.dp

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            val list: @Composable (Modifier) -> Unit = { modifier ->
                Column(modifier, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Label("Today", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                    if (reads) {
                        Table(
                            listOf(Col("Puzzle", 100.dp, weight = 1f), Col("Today", 160.dp, preferred = 200.dp), Col("Open", 80.dp, preferred = 90.dp)),
                            Puzzles.size,
                            { row, column ->
                                val puzzle = Puzzles[row]
                                val today = summaries[puzzle.id]?.today
                                val status = if (today == null && puzzle.id in wellness.unreadable) UnreadableGame else puzzleStatus(today)

                                when (column) {
                                    0 -> TableCell.Text(puzzle.name)
                                    1 -> TableCell.Slot(status, Tokens.Size.IconSm + Tokens.Space.S1_5) {
                                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                            Icon(if (today?.solved == true) MyIcon.Check else if (today == null) MyIcon.Minus else MyIcon.Review, if (today?.solved == true) colors.statusDone else colors.textMuted, Tokens.Size.IconSm)
                                            Label(status, color = colors.textMuted)
                                        }
                                    }
                                    else -> TableCell.Slot("Open", 0.dp) { MyButton({ onPlay(puzzle) }, text = "Open", description = "Open ${puzzle.name}", variant = ButtonVariant.Outline, size = ButtonSize.Sm) }
                                }
                            },
                            rowKey = { Puzzles[it].id },
                            onRow = { chosen = Puzzles[it].id },
                            rowDescription = { Puzzles[it].name },
                            rowModifier = { if (Puzzles[it].id == chosen) Modifier.background(colors.bgSelected) else Modifier },
                        )

                        Label("Results are read from each game when you open it here. Mycelium reads only these four sites, and only the day's game.", style = Tokens.Type.Meta, color = colors.textMuted)
                    } else {
                        Table(
                            listOf(Col("Puzzle", 100.dp, weight = 1f), Col("Open", 80.dp, preferred = 90.dp), Col("Played", 90.dp, preferred = 100.dp)),
                            Puzzles.size,
                            { row, column ->
                                val puzzle = Puzzles[row]
                                val played = summaries[puzzle.id]?.today != null

                                when (column) {
                                    0 -> TableCell.Text(puzzle.name, color = if (played) colors.textMuted else colors.text)
                                    1 -> TableCell.Slot("Open", 0.dp) { MyButton({ platform.openUrl(puzzle.url) }, text = "Open", description = "Open ${puzzle.name}", variant = ButtonVariant.Outline, size = ButtonSize.Sm) }
                                    else -> TableCell.Slot("Played", Tokens.Size.ControlMd) { Checkbox(played, "${puzzle.name} played today", { store.wellness.setPlayed(puzzle.id, !played) }, checkedState = "Played", uncheckedState = "Not played") }
                                }
                            },
                            rowKey = { Puzzles[it].id },
                            rowDescription = { Puzzles[it].name },
                        )

                        Label("Opens in your browser. Tick a puzzle once you have played it; the desktop app reads results from the game itself.", style = Tokens.Type.Meta, color = colors.textMuted)
                    }
                }
            }

            if (wide) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
                    list(Modifier.width(PuzzleListWidth))
                    PuzzleDetail(store, selected, summaries[selected.id], Modifier.weight(1f)) { onPlay(selected) }
                }
            } else {
                list(Modifier.fillMaxWidth())

                if (reads) {
                    PuzzleDetail(store, selected, summaries[selected.id], Modifier.fillMaxWidth()) { onPlay(selected) }
                }
            }
        }
    }
}

private fun averageLabel(puzzle: Puzzle): String = if (puzzle.measure == "mistakes") "Average mistakes" else "Average guesses"

@Composable
private fun PuzzleDetail(store: Store, puzzle: Puzzle, summary: PuzzleSummary?, modifier: Modifier, onOpen: () -> Unit) {
    val wellness by store.wellness.state.collectAsState()
    val colors = MyTheme.colors
    val stats = summary?.stats
    val history = summary?.history.orEmpty()
    val unreadable = summary?.today == null && puzzle.id in wellness.unreadable

    Column(modifier, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label(puzzle.name, Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)
            MyButton(onOpen, text = "Open ${puzzle.name}", variant = ButtonVariant.Primary)
        }

        if (unreadable) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Checkbox(false, "${puzzle.name} played today", { store.wellness.setPlayed(puzzle.id, true) }, checkedState = "Played", uncheckedState = "Not played")
                Label("Played today", color = colors.textMuted)
            }
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
            Stat("Played, last 30 days", (stats?.played ?: 0).toString())
            Stat("Solved", (stats?.solved ?: 0).toString())
            Stat(averageLabel(puzzle), stats?.average?.let { ((it * 10).toInt() / 10.0).toString() } ?: "None")
        }

        val distribution = stats?.distribution.orEmpty()

        if (distribution.isNotEmpty()) {
            val peak = distribution.maxOf { it.count }.coerceAtLeast(1)

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                Label(if (puzzle.measure == "mistakes") "Mistakes, last 30 days" else "Guesses, last 30 days", Modifier.semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)

                for (bucket in distribution) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                        Label(bucket.value.toString(), Modifier.width(24.dp), Tokens.Type.Meta, colors.textMuted)
                        Meter((bucket.count * 100 / peak).toInt(), "${bucket.value}: ${bucket.count}", Modifier.weight(1f))
                        Label(bucket.count.toString(), Modifier.width(32.dp), Tokens.Type.Meta, colors.textMuted)
                    }
                }
            }
        }

        Label("History", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        if (history.isEmpty()) {
            Label("Nothing recorded yet.", color = colors.textMuted)
        } else {
            Table(
                listOf(Col("Day", 90.dp, preferred = 110.dp), Col("Result", 140.dp, weight = 1f), Col("Recorded", 120.dp, preferred = 150.dp, priority = 1)),
                history.size,
                { row, column ->
                    val day = history[row]

                    when (column) {
                        0 -> TableCell.Text(dayLabel(day.day, today()))
                        1 -> TableCell.Text(puzzleStatus(day.result))
                        else -> TableCell.Text(day.result.sourceLabel(), color = colors.textMuted)
                    }
                },
                rowKey = { history[it].day },
                rowDescription = { dayLabel(history[it].day, today()) },
            )
        }
    }
}

@Composable
private fun Stat(name: String, value: String) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
        Label(name, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        Label(value, style = Tokens.Type.Title)
    }
}

@Composable
fun PuzzleBrowser(store: Store, platform: Platform, puzzle: Puzzle, onBack: () -> Unit) {
    val wellness by store.wellness.state.collectAsState()
    val recorded = wellness.puzzles.firstOrNull { it.puzzle == puzzle.id }?.today
    val colors = MyTheme.colors
    var read by remember { mutableStateOf<PuzzleResult?>(null) }
    val shown = read ?: recorded
    val status = if (shown == null) "Waiting for the game" else "${puzzleStatus(shown)}, recorded"
    val reading = remember(puzzle) {
        PageReading(
            script = puzzle.readingScript(),
            allowedUrl = { puzzle.isOnThisSite(it) },
            onRead = { storage ->
                val result = parseGameState(puzzle, storage) { playedAt -> Instant.fromEpochMilliseconds(playedAt).toString().take(10) == today() }

                if (result != null && result != read) {
                    read = result
                    store.wellness.recordResult(puzzle.id, result)
                }
            },
        )
    }

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(bottom = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MyButton({ if (read == null && recorded == null) store.wellness.markUnreadable(puzzle.id); onBack() }, text = "Back to puzzles", icon = MyIcon.ChevronLeft, muted = true)
            Icon(MyIcon.Globe, colors.textMuted, Tokens.Size.IconMd)
            Label(puzzle.name, style = Tokens.Type.UiStrong)
            Label(puzzle.host + puzzle.pathPrefix.trimEnd('/'), Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
            Label(status, Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
            MyButton({ platform.openUrl(puzzle.url) }, text = "Open in browser", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        Hairline()

        platform.WebContent(puzzle.url, null, Modifier.weight(1f).fillMaxWidth(), reading)
    }
}
