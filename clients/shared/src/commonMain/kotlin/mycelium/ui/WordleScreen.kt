package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
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
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.key.utf16CodePoint
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.api.WordleGame
import mycelium.state.Puzzle
import mycelium.state.Store
import mycelium.state.markPhrase
import mycelium.state.wordleBoardDescription
import mycelium.state.wordleGuessDescription
import mycelium.state.wordleKeyMarks

private const val WordLength = 5
private const val GuessLimit = 6
private val KeyRows = listOf("QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM")
private val BoardWidth = 360.dp

@Composable
fun WordleScreen(store: Store, puzzle: Puzzle, onBack: () -> Unit, onPlayOnSite: () -> Unit) {
    val wellness by store.wellness.state.collectAsState()
    var game by remember { mutableStateOf<WordleGame?>(null) }
    var failed by remember { mutableStateOf(false) }
    var tries by remember { mutableIntStateOf(0) }
    var typed by remember { mutableStateOf("") }
    var message by remember { mutableStateOf("") }
    var announced by remember { mutableStateOf("") }
    val runner = rememberAction()
    val focus = remember { FocusRequester() }
    val loaded = game
    val playing = loaded?.status == "playing"

    LaunchedEffect(tries) {
        failed = false
        failed = attempt { game = store.api.wordleGame() } != null
    }

    LaunchedEffect(loaded?.status) {
        if (loaded != null && loaded.status != "playing") {
            store.wellness.loadPuzzles()
        }

        if (loaded != null) {
            runCatching { focus.requestFocus() }
        }
    }

    if (failed && loaded == null) {
        PuzzleLoadFailed(puzzle, { tries++ }, onPlayOnSite, onBack)

        return
    }

    val typeLetter: (Char) -> Unit = { letter ->
        if (playing && typed.length < WordLength) {
            typed += letter.lowercaseChar()
            message = ""
            runner.failure = null
        }
    }
    val delete: () -> Unit = {
        typed = typed.dropLast(1)
        message = ""
        runner.failure = null
    }
    val submit: () -> Unit = {
        if (playing) {
            if (typed.length < WordLength) {
                message = "Enter five letters"
            } else {
                val word = typed

                runner {
                    val after = store.api.guessWordle(word)

                    game = after
                    typed = ""
                    message = ""
                    announced = wordleGuessDescription(after.guesses.lastIndex, after.guesses.last())
                }
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        PuzzleBar(puzzle.name, loaded?.day, if (loaded != null && playing) "Guess ${minOf(loaded.guesses.size + 1, GuessLimit)} of $GuessLimit" else "", onBack)

        if (loaded == null) {
            Label("Loading today's ${puzzle.name}", Modifier.padding(Tokens.Space.S4), color = MyTheme.colors.textMuted)

            return@Column
        }

        Column(
            Modifier
                .weight(1f)
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S6)
                .onKeyEvent { event -> handleKey(event, typeLetter, delete, submit) },
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4),
        ) {
            WordleBoard(loaded, typed, focus)

            val status = runner.failure ?: message

            Label(
                status,
                Modifier.semantics {
                    liveRegion = LiveRegionMode.Polite
                    contentDescription = status.ifEmpty { announced }
                },
                Tokens.Type.Meta,
                if (runner.failure != null) MyTheme.colors.statusFailed else MyTheme.colors.textMuted,
            )

            if (playing) {
                WordleKeyboard(loaded, typeLetter, delete, submit)

                Label("Type to guess, Enter to submit, Backspace to delete.", style = Tokens.Type.Meta, color = MyTheme.colors.textFaint)
            } else {
                val won = loaded.status == "won"
                val summary = wellness.puzzles.firstOrNull { it.puzzle == puzzle.id }

                PuzzleFinished(
                    headline = if (won) "Solved in ${loaded.guesses.size} ${if (loaded.guesses.size == 1) "guess" else "guesses"}" else "Not solved",
                    seconds = loaded.seconds,
                    summary = summary,
                    onBack = onBack,
                    solved = won,
                    detail = loaded.answer?.takeUnless { won }?.let { "The word was ${it.uppercase()}." },
                )
            }
        }
    }
}

private fun handleKey(event: KeyEvent, typeLetter: (Char) -> Unit, delete: () -> Unit, submit: () -> Unit): Boolean {
    if (event.type != KeyEventType.KeyDown || event.isCtrlPressed || event.isMetaPressed || event.isAltPressed) {
        return false
    }

    val character = event.utf16CodePoint.toChar()

    return when {
        event.key == Key.Enter || event.key == Key.NumPadEnter -> {
            submit()

            true
        }
        event.key == Key.Backspace -> {
            delete()

            true
        }
        character.isLetter() && character.code < 128 -> {
            typeLetter(character)

            true
        }
        else -> false
    }
}

@Composable
private fun WordleBoard(game: WordleGame, typed: String, focus: FocusRequester) {
    val colors = MyTheme.colors
    val size = if (LocalTouch.current) 52.dp else 56.dp
    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()
    val playing = game.status == "playing"

    Column(
        Modifier
            .widthIn(max = BoardWidth)
            .focusRequester(focus)
            .focusRing(focused, BlockShape)
            .focusable(interactionSource = source)
            .semantics { contentDescription = wordleBoardDescription(game.guesses) }
            .padding(Tokens.Space.S1_5),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
    ) {
        for (row in 0 until GuessLimit) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                val guess = game.guesses.getOrNull(row)
                val current = playing && row == game.guesses.size

                for (column in 0 until WordLength) {
                    when {
                        guess != null -> WordleTile(guess.word[column], guess.marks[column], false, size)
                        current -> WordleTile(typed.getOrNull(column), null, typed.getOrNull(column) != null, size)
                        else -> WordleTile(null, null, false, size)
                    }
                }
            }
        }
    }
}

@Composable
private fun markColor(mark: String?): Color? {
    val colors = MyTheme.colors

    return when (mark) {
        "correct" -> colors.statusDone
        "present" -> colors.statusAttention
        "absent" -> colors.bgSelected
        else -> null
    }
}

private fun markIcon(mark: String?): MyIcon? = when (mark) {
    "correct" -> MyIcon.Check
    "present" -> MyIcon.Move
    "absent" -> MyIcon.Minus
    else -> null
}

@Composable
private fun WordleTile(letter: Char?, mark: String?, typed: Boolean, size: Dp) {
    val colors = MyTheme.colors
    val fill = markColor(mark)
    val ink = when (mark) {
        null -> colors.text
        "absent" -> colors.textMuted
        else -> colors.onAccent
    }
    val edge = fill ?: if (typed) colors.borderControl else colors.border

    Box(
        Modifier
            .size(size)
            .then(if (fill != null) Modifier.background(fill, ControlShape) else Modifier)
            .border(Tokens.Size.FocusRing, edge, ControlShape)
            .clearAndSetSemantics { },
        contentAlignment = Alignment.Center,
    ) {
        if (letter != null) {
            Label(letter.uppercase(), style = Tokens.Type.Display, color = ink)
        }

        markIcon(mark)?.let { Box(Modifier.align(Alignment.TopEnd).padding(Tokens.Space.S0_5)) { Icon(it, ink, 12.dp) } }
    }
}

@Composable
private fun WordleKeyboard(game: WordleGame, typeLetter: (Char) -> Unit, delete: () -> Unit, submit: () -> Unit) {
    val marks = wordleKeyMarks(game.guesses)
    val keyHeight = 52.dp

    Column(
        Modifier.widthIn(max = 520.dp).fillMaxWidth().semantics { contentDescription = "Keyboard" },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
    ) {
        KeyRows.forEachIndexed { index, letters ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1, Alignment.CenterHorizontally)) {
                if (index == KeyRows.lastIndex) {
                    WordleKey("Enter", "Enter", null, keyHeight, Modifier.weight(1.5f), submit)
                }

                for (letter in letters) {
                    val mark = marks[letter.lowercaseChar()]
                    val name = if (mark == null) letter.toString() else "$letter, ${markPhrase(mark)}"

                    WordleKey(letter.toString(), name, mark, keyHeight, Modifier.weight(1f)) { typeLetter(letter) }
                }

                if (index == KeyRows.lastIndex) {
                    WordleKey("Delete", "Delete", null, keyHeight, Modifier.weight(1.5f), delete)
                }
            }
        }
    }
}

@Composable
private fun WordleKey(label: String, name: String, mark: String?, height: Dp, modifier: Modifier, onClick: () -> Unit) {
    val colors = MyTheme.colors
    val fill = markColor(mark) ?: colors.bgHover
    val ink = when (mark) {
        null -> colors.text
        "absent" -> colors.textMuted
        else -> colors.onAccent
    }

    Box(
        modifier
            .height(height)
            .border(Tokens.Size.Hairline, markColor(mark) ?: colors.borderControl, ControlShape)
            .pressable(onClick, description = name, shape = ControlShape, base = fill, ringInside = true),
        contentAlignment = Alignment.Center,
    ) {
        Label(label, style = if (label.length > 1) Tokens.Type.Meta else Tokens.Type.UiStrong, color = ink, maxLines = 1)

        markIcon(mark)?.let { Box(Modifier.align(Alignment.TopEnd).padding(Tokens.Space.S0_5)) { Icon(it, ink, 10.dp) } }
    }
}
