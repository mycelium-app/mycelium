package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
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
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import mycelium.Platform
import mycelium.PageReading
import mycelium.api.PuzzleResult
import mycelium.api.PuzzleSummary
import mycelium.state.GameReading
import mycelium.state.Puzzle
import mycelium.state.Puzzles
import mycelium.state.Store
import mycelium.state.averageLabel
import mycelium.state.daysText
import mycelium.state.distributionRange
import mycelium.state.distributionTitle
import mycelium.state.isOnThisSite
import mycelium.state.parseGameState
import mycelium.state.puzzleStatus
import mycelium.state.readingScript
import mycelium.state.scoreLabel
import mycelium.state.secondsText

private const val UnreadableGame = "Could not read the game"
private val PuzzleListWidth = 520.dp

@Composable
fun PuzzlesTab(store: Store, platform: Platform, onPlay: (Puzzle) -> Unit) {
    val wellness by store.wellness.state.collectAsState()
    var chosen by remember { mutableStateOf(Puzzles.first().id) }
    var viewing by remember { mutableStateOf<Puzzle?>(null) }
    var recording by remember { mutableStateOf<Puzzle?>(null) }
    val summaries = wellness.puzzles.associateBy { it.puzzle }
    val reads = platform.readsPages
    val open = { puzzle: Puzzle -> if (puzzle.native || reads) onPlay(puzzle) else platform.openUrl(puzzle.url) }
    val history = viewing

    LaunchedEffect(Unit) {
        store.wellness.loadPuzzles()
    }

    if (history != null && !reads) {
        PuzzleHistory(history, summaries[history.id], { viewing = null }, { open(history) }, { recording = history })
    } else {
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val wide = reads && maxWidth >= 900.dp
            val selected = Puzzles.first { it.id == chosen }
            val detail: @Composable (Modifier) -> Unit = { modifier ->
                val unreadable = summaries[selected.id]?.today == null && selected.id in wellness.unreadable

                PuzzleDetail(selected, summaries[selected.id], modifier, unreadable, { open(selected) }, { recording = selected })
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
                if (wide) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
                        TodayTable(Modifier.width(PuzzleListWidth), reads, summaries, wellness.unreadable, chosen, { chosen = it.id }, open, { viewing = it }, { recording = it })
                        detail(Modifier.weight(1f))
                    }
                } else {
                    TodayTable(Modifier.fillMaxWidth(), reads, summaries, wellness.unreadable, chosen, { chosen = it.id }, open, { viewing = it }, { recording = it })

                    if (reads) {
                        detail(Modifier.fillMaxWidth())
                    }
                }
            }
        }
    }

    recording?.let { RecordResultDialog(store, it) { recording = null } }
}

private fun todayText(puzzle: Puzzle, result: PuzzleResult): String =
    (puzzle.scoreLabel(result)?.takeIf { it != "X" } ?: puzzleStatus(puzzle, result)).replaceFirstChar { it.uppercase() }

@Composable
private fun TodayTable(
    modifier: Modifier,
    reads: Boolean,
    summaries: Map<String, PuzzleSummary>,
    unreadable: Set<String>,
    chosen: String,
    onChoose: (Puzzle) -> Unit,
    onOpen: (Puzzle) -> Unit,
    onHistory: (Puzzle) -> Unit,
    onRecord: (Puzzle) -> Unit,
) {
    val colors = MyTheme.colors

    Column(modifier, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Today", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        if (reads) {
            Table(
                listOf(Col("Puzzle", 100.dp, weight = 1f), Col("Today", 160.dp, preferred = 200.dp), Col("Streak", 70.dp, preferred = 80.dp, priority = 1), Col("Open", 80.dp, preferred = 90.dp)),
                Puzzles.size,
                { row, column ->
                    val puzzle = Puzzles[row]
                    val summary = summaries[puzzle.id]
                    val today = summary?.today
                    val status = if (today == null && puzzle.id in unreadable) UnreadableGame else puzzleStatus(puzzle, today)

                    when (column) {
                        0 -> TableCell.Text(puzzle.name)
                        1 -> TableCell.Slot(status, Tokens.Size.IconSm + Tokens.Space.S1_5) {
                            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                Icon(if (today?.solved == true) MyIcon.Check else if (today == null) MyIcon.Minus else MyIcon.Review, if (today?.solved == true) colors.statusDone else colors.textMuted, Tokens.Size.IconSm)
                                Label(status, color = colors.textMuted)
                            }
                        }
                        2 -> TableCell.Text(daysText(summary?.stats?.streak ?: 0), color = colors.textMuted)
                        else -> TableCell.Slot("Open", 0.dp) { MyButton({ onOpen(puzzle) }, text = "Open", description = "Open ${puzzle.name}", variant = ButtonVariant.Outline, size = ButtonSize.Sm) }
                    }
                },
                rowKey = { Puzzles[it].id },
                onRow = { onChoose(Puzzles[it]) },
                rowDescription = { Puzzles[it].name },
                rowModifier = { if (Puzzles[it].id == chosen) Modifier.background(colors.bgSelected) else Modifier },
            )

            Label("Wordle and Connections are played here. The other results are read from each game when you open it here; Mycelium reads only those three sites, and only the day's game.", style = Tokens.Type.Meta, color = colors.textMuted)
        } else {
            Table(
                listOf(Col("Puzzle", 100.dp, weight = 1f), Col("Today", 90.dp, preferred = 120.dp), Col("Open", 80.dp, preferred = 90.dp)),
                Puzzles.size,
                { row, column ->
                    val puzzle = Puzzles[row]
                    val today = summaries[puzzle.id]?.today

                    when (column) {
                        0 -> TableCell.Slot(puzzle.name, 0.dp) { MyButton({ onHistory(puzzle) }, text = puzzle.name, description = "${puzzle.name}, history", muted = true) }
                        1 -> when {
                            today != null -> TableCell.Text(todayText(puzzle, today), color = colors.textMuted)
                            puzzle.native -> TableCell.Text("Not played", color = colors.textMuted)
                            else -> TableCell.Slot("Record", 0.dp) { MyButton({ onRecord(puzzle) }, text = "Record", description = "Record a result for ${puzzle.name}", variant = ButtonVariant.Outline, size = ButtonSize.Sm) }
                        }
                        else -> TableCell.Slot("Open", 0.dp) { MyButton({ onOpen(puzzle) }, text = "Open", description = "Open ${puzzle.name}", variant = ButtonVariant.Outline, size = ButtonSize.Sm) }
                    }
                },
                rowKey = { Puzzles[it].id },
                rowDescription = { Puzzles[it].name },
            )

            Label("Wordle and Connections are played here. The others open in your browser; record your result when you come back.", style = Tokens.Type.Meta, color = colors.textMuted)
        }
    }
}

@Composable
private fun PuzzleHistory(puzzle: Puzzle, summary: PuzzleSummary?, onBack: () -> Unit, onOpen: () -> Unit, onRecord: () -> Unit) {
    val colors = MyTheme.colors
    val today = summary?.today

    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            MyButton(onBack, icon = MyIcon.ChevronLeft, description = "Back to puzzles", muted = true)
            Label(puzzle.name, Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)
            MyButton(onOpen, text = "Open", description = "Open ${puzzle.name}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(if (today == null) MyIcon.Minus else MyIcon.Check, if (today?.solved == true) colors.statusDone else colors.textMuted, Tokens.Size.IconSm)
            Label(if (today == null) "Today: not recorded" else "Today: ${puzzleStatus(puzzle, today)}", Modifier.weight(1f))

            if (today == null && !puzzle.native) {
                MyButton(onRecord, text = "Record a result", variant = ButtonVariant.Primary)
            }
        }

        PuzzleDetailBody(puzzle, summary)
    }
}

@Composable
private fun PuzzleDetail(puzzle: Puzzle, summary: PuzzleSummary?, modifier: Modifier, canRecord: Boolean, onOpen: () -> Unit, onRecord: () -> Unit) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label(puzzle.name, Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)

            if (canRecord) {
                MyButton(onRecord, text = "Record a result", variant = ButtonVariant.Outline)
            }

            MyButton(onOpen, text = "Open ${puzzle.name}", variant = ButtonVariant.Primary)
        }

        PuzzleDetailBody(puzzle, summary)
    }
}

@Composable
private fun PuzzleDetailBody(puzzle: Puzzle, summary: PuzzleSummary?) {
    val colors = MyTheme.colors
    val stats = summary?.stats
    val today = summary?.day.orEmpty().ifEmpty { kotlin.time.Clock.System.now().toString().take(10) }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            PuzzleStat("Streak", daysText(stats?.streak ?: 0), "Best ${daysText(stats?.bestStreak ?: 0)}")
            PuzzleStat("Solved, last 30 days", "${stats?.solved ?: 0} of ${stats?.played ?: 0}")
            PuzzleStat(puzzle.averageLabel(), stats?.average?.let { ((it * 10).roundToInt() / 10.0).toString() } ?: "None")
            PuzzleStat("Average time", stats?.averageSeconds?.let { secondsText(it.roundToInt()) } ?: "None")
        }

        PuzzleCalendar(puzzle, summary?.history.orEmpty(), today)

        val range = puzzle.distributionRange()

        if (range != null && stats != null) {
            val counts = stats.distribution.associate { it.value to it.count }
            val peak = (counts.values.maxOrNull() ?: 0L).coerceAtLeast(1L)
            val singular = if (puzzle.measure == "mistakes") "mistake" else "guess"
            val plural = if (puzzle.measure == "mistakes") "mistakes" else "guesses"
            val summaryText = "${puzzle.distributionTitle()}: " + range.joinToString(", ") { "$it ${if (it == 1) singular else plural} ${counts[it] ?: 0}" }

            Column(Modifier.clearAndSetSemantics { contentDescription = summaryText }, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                Label(puzzle.distributionTitle(), style = Tokens.Type.MetaStrong, color = colors.textMuted)

                for (value in range) {
                    val count = counts[value] ?: 0L

                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                        Label(value.toString(), Modifier.width(24.dp), Tokens.Type.Meta, colors.textMuted)
                        Meter((count * 100 / peak).toInt(), "$value: $count", Modifier.weight(1f))
                        Label(count.toString(), Modifier.width(32.dp), Tokens.Type.Meta, colors.textMuted)
                    }
                }
            }
        }
    }
}

@Composable
fun PuzzlePlay(store: Store, platform: Platform, puzzle: Puzzle, onBack: () -> Unit) {
    var onSite by remember(puzzle.id) { mutableStateOf(false) }
    var recording by remember(puzzle.id) { mutableStateOf(false) }
    val playOnSite: () -> Unit = {
        if (platform.readsPages) {
            onSite = true
        } else {
            platform.openUrl(puzzle.url)
            recording = true
        }
    }

    when {
        puzzle.id == "wordle" && !onSite -> WordleScreen(store, puzzle, onBack, playOnSite)
        puzzle.id == "connections" && !onSite -> ConnectionsScreen(store, puzzle, onBack, playOnSite)
        else -> PuzzleBrowser(store, platform, puzzle, onBack)
    }

    if (recording) {
        RecordResultDialog(store, puzzle) { recording = false }
    }
}

@Composable
fun PuzzleBrowser(store: Store, platform: Platform, puzzle: Puzzle, onBack: () -> Unit) {
    val wellness by store.wellness.state.collectAsState()
    val recorded = wellness.puzzles.firstOrNull { it.puzzle == puzzle.id }?.today
    val colors = MyTheme.colors
    var read by remember { mutableStateOf<PuzzleResult?>(null) }
    val shown = read ?: recorded
    val today = wellness.puzzles.firstOrNull()?.day.orEmpty()
    val status = when {
        shown != null -> "${puzzleStatus(puzzle, shown)}, recorded"
        puzzle.id in wellness.unreadable -> UnreadableGame
        else -> "Waiting for the game"
    }

    LaunchedEffect(puzzle) {
        store.wellness.loadPuzzles()
    }

    val reading = remember(puzzle, today) {
        if (today.isEmpty()) {
            null
        } else {
            PageReading(
                script = puzzle.readingScript(),
                allowedUrl = { puzzle.isOnThisSite(it) },
                onRead = { storage ->
                    when (val game = parseGameState(puzzle, storage, today)) {
                        GameReading.NoStorage -> Unit
                        GameReading.Unrecognised -> store.wellness.markUnreadable(puzzle.id)
                        is GameReading.Read -> {
                            store.wellness.markReadable(puzzle.id)

                            if (game.result != null && game.result != read) {
                                read = game.result
                                store.wellness.recordResult(puzzle.id, game.result)
                            }
                        }
                    }
                },
            )
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            MyButton(onBack, icon = MyIcon.ChevronLeft, description = "Back to puzzles", muted = true)
            Icon(MyIcon.Globe, colors.textMuted, Tokens.Size.IconMd)
            Label(puzzle.name, style = Tokens.Type.UiStrong)
            Label(puzzle.host + puzzle.pathPrefix.trimEnd('/'), Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
            Label(status, Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
            MyButton({ platform.openUrl(puzzle.url) }, text = "Open in browser", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        platform.WebContent(puzzle.url, null, Modifier.weight(1f).fillMaxWidth(), reading)
    }
}
