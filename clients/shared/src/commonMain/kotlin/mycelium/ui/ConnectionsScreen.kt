package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.background
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.unit.dp
import mycelium.api.ConnectionsGame
import mycelium.api.ConnectionsGroup
import mycelium.state.Puzzle
import mycelium.state.Store

private const val GroupSize = 4
private val BoardMaxWidth = 640.dp
private val WordHeight = 72.dp
private val WordHeightTouch = 60.dp

private fun outcomeText(outcome: String): String = when (outcome) {
    "correct" -> "Correct"
    "one_away" -> "One away"
    "already_guessed" -> "Already guessed"
    else -> "Incorrect"
}

@Composable
private fun levelColor(level: Int): Color {
    val colors = MyTheme.colors

    return when (level) {
        0 -> colors.statusAttention
        1 -> colors.statusDone
        2 -> colors.statusWorking
        else -> colors.statusFailed
    }
}

@Composable
fun ConnectionsScreen(store: Store, puzzle: Puzzle, onBack: () -> Unit, onPlayOnSite: () -> Unit) {
    val wellness by store.wellness.state.collectAsState()
    var game by remember { mutableStateOf<ConnectionsGame?>(null) }
    var failed by remember { mutableStateOf(false) }
    var tries by remember { mutableIntStateOf(0) }
    var picked by remember { mutableStateOf(emptyList<String>()) }
    var shuffled by remember { mutableStateOf<List<String>?>(null) }
    var message by remember { mutableStateOf("") }
    val runner = rememberAction()
    val loaded = game
    val playing = loaded?.status == "playing"

    LaunchedEffect(tries) {
        failed = false
        failed = attempt { game = store.api.connectionsGame() } != null
    }

    LaunchedEffect(loaded?.status) {
        if (loaded != null && loaded.status != "playing") {
            store.wellness.loadPuzzles()
        }
    }

    if (failed && loaded == null) {
        PuzzleLoadFailed(puzzle, { tries++ }, onPlayOnSite, onBack)

        return
    }

    val toggle: (String) -> Unit = { word ->
        picked = if (word in picked) picked - word else if (picked.size < GroupSize) picked + word else picked
        message = ""
    }
    val submit: () -> Unit = {
        if (playing) {
            if (picked.size != GroupSize) {
                message = "Select four words"
            } else {
                val words = picked

                runner {
                    val answer = store.api.guessConnections(words)

                    game = answer.game
                    message = outcomeText(answer.outcome)

                    if (answer.outcome != "already_guessed") {
                        picked = emptyList()
                    }
                }
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        PuzzleBar(puzzle.name, loaded?.day, "", onBack)

        if (loaded == null) {
            Label("Loading today's ${puzzle.name}", Modifier.padding(Tokens.Space.S4), color = MyTheme.colors.textMuted)

            return@Column
        }

        val colors = MyTheme.colors
        val remaining = shuffled?.filter { it in loaded.words } ?: loaded.words

        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S6),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4),
        ) {
            WordGrid(loaded, remaining, picked, toggle, submit)

            if (playing) {
                val left = loaded.mistakesAllowed - loaded.mistakes

                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    Label("Mistakes left: $left")

                    Row(Modifier.clearAndSetSemantics { }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                        for (index in 0 until loaded.mistakesAllowed) {
                            Box(Modifier.size(10.dp).background(if (index < left) colors.textMuted else Color.Transparent, CircleShape).border(Tokens.Size.Hairline, if (index < left) colors.textMuted else colors.borderControl, CircleShape))
                        }
                    }
                }

                Label(message, Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Ui)

                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.CenterHorizontally), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    MyButton({ shuffled = remaining.shuffled() }, text = "Shuffle", variant = ButtonVariant.Outline)
                    MyButton({ picked = emptyList(); message = "" }, text = "Deselect all", variant = ButtonVariant.Outline)
                    MyButton({ submit() }, text = "Submit", variant = ButtonVariant.Primary)
                }

                runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

                Label("Arrow keys move, Space selects a word, Enter submits four.", style = Tokens.Type.Meta, color = colors.textFaint)
            } else {
                val won = loaded.status == "won"
                val summary = wellness.puzzles.firstOrNull { it.puzzle == puzzle.id }
                val mistakes = when (loaded.mistakes) {
                    0 -> "no mistakes"
                    1 -> "1 mistake"
                    else -> "${loaded.mistakes} mistakes"
                }

                PuzzleFinished(
                    headline = if (won) "Solved, $mistakes" else "Not solved",
                    seconds = loaded.seconds,
                    summary = summary,
                    onBack = onBack,
                    solved = won,
                )
            }
        }
    }
}

@Composable
private fun WordGrid(game: ConnectionsGame, words: List<String>, picked: List<String>, toggle: (String) -> Unit, submit: () -> Unit) {
    val focusManager = LocalFocusManager.current
    val height = if (LocalTouch.current) WordHeightTouch else WordHeight
    val playing = game.status == "playing"

    Column(
        Modifier
            .widthIn(max = BoardMaxWidth)
            .fillMaxWidth()
            .semantics { contentDescription = "Words. Arrow keys move, Space selects, Enter submits four." }
            .onPreviewKeyEvent { event ->
                val direction = when (event.key) {
                    Key.DirectionLeft -> FocusDirection.Left
                    Key.DirectionRight -> FocusDirection.Right
                    Key.DirectionUp -> FocusDirection.Up
                    Key.DirectionDown -> FocusDirection.Down
                    else -> null
                }

                when {
                    event.type != KeyEventType.KeyDown -> false
                    direction != null -> focusManager.moveFocus(direction)
                    event.key == Key.Enter || event.key == Key.NumPadEnter -> {
                        submit()

                        true
                    }
                    else -> false
                }
            },
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        for (group in game.solved + game.unsolved) {
            GroupRow(group, height)
        }

        if (playing) {
            for (row in words.chunked(GroupSize)) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    for (word in row) {
                        WordTile(word, word in picked, height, Modifier.weight(1f)) { toggle(word) }
                    }
                }
            }
        }
    }
}

@Composable
private fun GroupRow(group: ConnectionsGroup, height: androidx.compose.ui.unit.Dp) {
    val colors = MyTheme.colors

    Column(
        Modifier
            .fillMaxWidth()
            .height(height)
            .background(levelColor(group.level), BlockShape)
            .clearAndSetSemantics { contentDescription = "Solved: ${group.title}, ${group.words.joinToString(", ")}" },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Label(group.title, style = Tokens.Type.Title, color = colors.onAccent, maxLines = 1)
        Label(group.words.joinToString(", "), style = Tokens.Type.Ui, color = colors.onAccent, maxLines = 1)
    }
}

@Composable
private fun WordTile(word: String, selected: Boolean, height: androidx.compose.ui.unit.Dp, modifier: Modifier, onToggle: () -> Unit) {
    val colors = MyTheme.colors
    val ink = if (selected) colors.onAccent else colors.text

    Box(
        modifier
            .height(height)
            .border(Tokens.Size.Hairline, if (selected) colors.accent else colors.borderControl, BlockShape)
            .onPreviewKeyEvent { event ->
                val space = event.type == KeyEventType.KeyDown && event.key == Key.Spacebar

                if (space) {
                    onToggle()
                }

                space
            }
            .pressable(onToggle, role = Role.Checkbox, shape = BlockShape, base = if (selected) colors.accent else colors.bgHover, ringInside = true)
            .semantics { toggleableState = if (selected) ToggleableState.On else ToggleableState.Off },
        contentAlignment = Alignment.Center,
    ) {
        Label(word, Modifier.padding(horizontal = Tokens.Space.S1), Tokens.Type.UiStrong, ink, maxLines = 2)

        if (selected) {
            Box(Modifier.align(Alignment.TopEnd).padding(Tokens.Space.S1_5)) { Icon(MyIcon.Check, ink, 14.dp) }
        }
    }
}
