package mycelium.state

import mycelium.api.PuzzleDay
import mycelium.api.PuzzleResult
import mycelium.api.PuzzleSummary
import mycelium.api.WordleGuess

private val CalendarMonths = listOf("January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December")
private const val SecondsPerMinute = 60
private const val OrbsToSolve = 6
private const val WordleGuessLimit = 6
private const val ConnectionsMistakeLimit = 4
private const val CatfishingQuestionCount = 10
private val MarkRank = mapOf("absent" to 1, "present" to 2, "correct" to 3)

fun secondsText(seconds: Int): String {
    val minutes = seconds / SecondsPerMinute
    val rest = seconds % SecondsPerMinute

    return if (minutes == 0) "$rest s" else "$minutes min ${rest.toString().padStart(2, '0')} s"
}

fun parseDuration(text: String): Int? {
    val parts = text.trim().split(':')
    val minutes = parts.getOrNull(0)?.toIntOrNull()
    val seconds = parts.getOrNull(1)?.toIntOrNull()

    if (parts.size != 2 || minutes == null || seconds == null || minutes < 0 || seconds !in 0 until SecondsPerMinute) {
        return null
    }

    return minutes * SecondsPerMinute + seconds
}

fun daysText(days: Long): String = if (days == 1L) "1 day" else "$days days"

fun puzzlesToday(summaries: List<PuzzleSummary>): Pair<Int, List<String>> {
    val played = summaries.associate { it.puzzle to (it.today != null) }
    val left = Puzzles.filter { played[it.id] != true }

    return (Puzzles.size - left.size) to left.map { it.name }
}

data class CalendarMonth(val year: Int, val month: Int) {
    val title: String get() = "${CalendarMonths[month - 1]} $year"

    val length: Int
        get() {
            val leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)

            return if (month == 2) (if (leap) 29 else 28) else if (month in listOf(4, 6, 9, 11)) 30 else 31
        }

    fun shifted(by: Int): CalendarMonth {
        val index = year * 12 + (month - 1) + by

        return CalendarMonth(index / 12, index % 12 + 1)
    }

    fun iso(day: Int): String = "${year.toString().padStart(4, '0')}-${month.toString().padStart(2, '0')}-${day.toString().padStart(2, '0')}"
}

data class CalendarCell(val day: Int, val iso: String, val entry: PuzzleDay?, val future: Boolean, val today: Boolean)

fun monthOf(isoDate: String): CalendarMonth {
    val (year, month) = isoDate.split('-').map { it.toInt() }

    return CalendarMonth(year, month)
}

fun CalendarMonth.cells(today: String, history: List<PuzzleDay>): List<CalendarCell?> {
    val byDay = history.associateBy { it.day }
    val blanks = List(weekdayOf(iso(1))) { null }
    val days = (1..length).map { day -> iso(day).let { CalendarCell(day, it, byDay[it], it > today, it == today) } }

    return blanks + days
}

fun CalendarMonth.canStepBack(history: List<PuzzleDay>): Boolean {
    val earliest = history.minOfOrNull { it.day } ?: return false

    return iso(1) > earliest
}

fun CalendarMonth.canStepForward(today: String): Boolean = iso(1) < today.take(8) + "01"

fun Puzzle.calendarSummary(result: PuzzleResult): String {
    val status = when (measure) {
        "guesses", "mistakes" -> puzzleStatus(this, result).replaceFirstChar { it.lowercaseChar() }
        else -> listOfNotNull(scoreLabel(result), solvedWord(result)).joinToString(", ").ifEmpty { "played" }
    }

    return if (result.source == "manual") "$status, entered by hand" else status
}

private fun solvedWord(result: PuzzleResult): String? = when (result.solved) {
    true -> "solved"
    false -> "not solved"
    null -> null
}

fun Puzzle.calendarDescription(cell: CalendarCell): String = "${dayLabel(cell.iso, "")}: ${cell.entry?.let { calendarSummary(it.result) } ?: "not played"}"

fun Puzzle.shortScore(result: PuzzleResult): String = result.score?.toString() ?: if (measure == "guesses" && result.solved == false) "X" else ""

fun Puzzle.averageLabel(): String = when (measure) {
    "guesses" -> "Average guesses"
    "mistakes" -> "Average mistakes"
    "orbs" -> "Average orbs"
    "correct" -> "Average correct"
    else -> "Average points"
}

fun Puzzle.distributionRange(): IntRange? = when (measure) {
    "guesses" -> 1..WordleGuessLimit
    "mistakes" -> 0..ConnectionsMistakeLimit
    else -> null
}

fun Puzzle.distributionTitle(): String = if (measure == "mistakes") "Mistakes, last 30 days" else "Guesses, last 30 days"

fun markPhrase(mark: String): String = when (mark) {
    "correct" -> "in the right place"
    "present" -> "in the word, wrong place"
    else -> "not in the word"
}

fun wordleGuessDescription(index: Int, guess: WordleGuess): String =
    "Guess ${index + 1}, ${guess.word}: " + guess.word.indices.joinToString("; ") { "${guess.word[it]} ${markPhrase(guess.marks[it])}" }

fun wordleBoardDescription(guesses: List<WordleGuess>): String =
    if (guesses.isEmpty()) "Board. No guesses yet" else "Board. " + guesses.mapIndexed { index, guess -> wordleGuessDescription(index, guess) }.joinToString("; ")

fun wordleKeyMarks(guesses: List<WordleGuess>): Map<Char, String> {
    val best = mutableMapOf<Char, String>()

    for (guess in guesses) {
        guess.word.forEachIndexed { index, letter ->
            val mark = guess.marks[index]

            if ((MarkRank[mark] ?: 0) > (MarkRank[best[letter]] ?: 0)) {
                best[letter] = mark
            }
        }
    }

    return best
}

fun wordleHardModeProblem(guesses: List<WordleGuess>, next: String): String? {
    for (guess in guesses) {
        for (index in guess.word.indices) {
            if (guess.marks[index] == "correct" && next.getOrNull(index) != guess.word[index]) {
                return "${ordinal(index + 1)} letter must be ${guess.word[index].uppercaseChar()}"
            }
        }
    }

    for (guess in guesses) {
        val wanted = guess.word.indices.filter { guess.marks[it] != "absent" }.map { guess.word[it] }
        val available = next.toMutableList()

        for (letter in wanted) {
            if (!available.remove(letter)) {
                return "Guess must contain ${letter.uppercaseChar()}"
            }
        }
    }

    return null
}

private fun ordinal(number: Int): String = when (number) {
    1 -> "1st"
    2 -> "2nd"
    3 -> "3rd"
    else -> "${number}th"
}

sealed interface RecordInput {
    data class Valid(val result: PuzzleResult) : RecordInput

    data class Invalid(val scoreError: String?, val timeError: String?) : RecordInput
}

fun Puzzle.recordLabel(): String = when (measure) {
    "guesses" -> "Guesses"
    "mistakes" -> "Mistakes"
    "orbs" -> "Orbs collected"
    "correct" -> "Correct answers"
    else -> "Points"
}

fun Puzzle.recordHint(): String = when (measure) {
    "guesses" -> "Enter 1 to 6, or X if you did not solve it."
    "mistakes" -> "Enter 0 to 4. Four mistakes ends the game."
    "orbs" -> "Six or more solves the day."
    "correct" -> "Out of $CatfishingQuestionCount."
    else -> "The score the game gave you."
}

private fun Puzzle.recordScoreError(): String = when (measure) {
    "guesses" -> "Enter the number of guesses, from 1 to 6, or X"
    "mistakes" -> "Enter the number of mistakes, from 0 to 4"
    "orbs" -> "Enter the number of orbs you collected"
    "correct" -> "Enter how many you got right, from 0 to $CatfishingQuestionCount"
    else -> "Enter the points the game gave you"
}

fun Puzzle.recordInput(scoreText: String, timeText: String): RecordInput {
    val trimmed = scoreText.trim()
    val number = trimmed.toIntOrNull()?.takeIf { it >= 0 }
    val timeBlank = timeText.isBlank()
    val seconds = if (timeBlank) null else parseDuration(timeText)
    val timeError = if (!timeBlank && seconds == null) "Enter the time like 4:30" else null

    val scored: PuzzleResult? = when (measure) {
        "guesses" -> if (trimmed.equals("x", ignoreCase = true)) PuzzleResult(solved = false) else number?.takeIf { it in 1..WordleGuessLimit }?.let { PuzzleResult(solved = true, score = it) }
        "mistakes" -> number?.takeIf { it <= ConnectionsMistakeLimit }?.let { PuzzleResult(solved = it < ConnectionsMistakeLimit, score = it) }
        "orbs" -> number?.let { PuzzleResult(solved = it >= OrbsToSolve, score = it) }
        "correct" -> number?.takeIf { it <= CatfishingQuestionCount }?.let { PuzzleResult(score = it) }
        else -> number?.let { PuzzleResult(score = it) }
    }

    if (scored == null || timeError != null) {
        return RecordInput.Invalid(if (scored == null) recordScoreError() else null, timeError)
    }

    return RecordInput.Valid(scored.copy(seconds = seconds))
}
