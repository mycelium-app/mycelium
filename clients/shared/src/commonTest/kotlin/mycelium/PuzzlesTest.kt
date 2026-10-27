package mycelium

import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.PuzzleDay
import mycelium.api.PuzzleResult
import mycelium.api.PuzzleSummary
import mycelium.api.WordleGuess
import mycelium.state.DiffLine
import mycelium.state.CalendarMonth
import mycelium.state.GameReading
import mycelium.state.Puzzles
import mycelium.state.RecordInput
import mycelium.state.calendarDescription
import mycelium.state.canStepBack
import mycelium.state.canStepForward
import mycelium.state.cells
import mycelium.state.isOnThisSite
import mycelium.state.parseDuration
import mycelium.state.parseGameState
import mycelium.state.puzzleOf
import mycelium.state.puzzleStatus
import mycelium.state.puzzlesToday
import mycelium.state.recordInput
import mycelium.state.secondsText
import mycelium.state.wordleBoardDescription
import mycelium.state.wordleGuessDescription
import mycelium.state.wordleKeyMarks
import mycelium.state.readingScript
import mycelium.state.solutionLines
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class PuzzlesTest {
    private val wordle = puzzleOf("wordle")!!
    private val connections = puzzleOf("connections")!!
    private val orbs = puzzleOf("daily-orbs")!!
    private val catfishing = puzzleOf("catfishing")!!
    private val enclose = puzzleOf("enclose-horse")!!
    private val today = "2026-10-27"

    private fun storage(key: String, value: String): String = """{"$key": ${JsonPrimitive(value)}}"""

    private fun nyt(game: String, printDate: String, data: String, extra: String = ""): String =
        storage("games-state-$game/ANON", """{"states":[{"puzzleId":1,"printDate":"$printDate","schemaVersion":"1.0","timestamp":1,"data":$data$extra}]}""")

    private fun read(result: PuzzleResult?) = GameReading.Read(result)

    @Test
    fun aWonWordleIsReadAsSolvedInTheRowsFilled() {
        val data = """{"boardState":["crane","slate","stale","","",""],"currentRowIndex":3,"status":"WIN","isPlayingArchive":false}"""

        assertEquals(read(PuzzleResult(solved = true, score = 3)), parseGameState(wordle, nyt("wordleV2", today, data), today))
    }

    @Test
    fun aLostWordleIsNotSolved() {
        val data = """{"boardState":["a","b","c","d","e","f"],"status":"FAIL"}"""

        assertEquals(read(PuzzleResult(solved = false)), parseGameState(wordle, nyt("wordleV2", today, data), today))
    }

    @Test
    fun aWordleStillInProgressIsPlayedButNotRecorded() {
        val data = """{"boardState":["crane","","","","",""],"status":"IN_PROGRESS"}"""

        assertEquals(read(null), parseGameState(wordle, nyt("wordleV2", today, data), today))
    }

    @Test
    fun aSignedInWordleIsReadFromTheirOwnKey() {
        val data = """{"boardState":["crane","slate","","","",""],"status":"WIN"}"""
        val signedIn = storage("games-state-wordleV2/48213", """{"states":[{"puzzleId":9,"printDate":"$today","data":$data}]}""")

        assertEquals(read(PuzzleResult(solved = true, score = 2)), parseGameState(wordle, signedIn, today))
    }

    @Test
    fun anArchivePuzzleOrAnotherDayIsNotToday() {
        val won = """{"boardState":["crane","","","","",""],"status":"WIN","isPlayingArchive":true}"""
        val earlier = """{"boardState":["crane","","","","",""],"status":"WIN"}"""

        assertEquals(read(null), parseGameState(wordle, nyt("wordleV2", today, won), today))
        assertEquals(read(null), parseGameState(wordle, nyt("wordleV2", "2026-10-26", earlier), today))
    }

    @Test
    fun aFinishedConnectionsCountsItsMistakes() {
        val won = """{"puzzleComplete":true,"puzzleWon":true,"mistakes":1,"guesses":[]}"""
        val lost = """{"puzzleComplete":true,"puzzleWon":false,"mistakes":4,"guesses":[]}"""
        val going = """{"puzzleComplete":false,"puzzleWon":false,"mistakes":2,"guesses":[]}"""

        assertEquals(read(PuzzleResult(solved = true, score = 1)), parseGameState(connections, nyt("connections", today, won), today))
        assertEquals(read(PuzzleResult(solved = false, score = 4)), parseGameState(connections, nyt("connections", today, lost), today))
        assertEquals(read(null), parseGameState(connections, nyt("connections", today, going), today))
    }

    @Test
    fun aDailyOrbsDayIsWonAtSixOrbsOrLostWhenAllFourPuzzlesAreDone() {
        val orb = """{"type":"orb","color":"red"}"""
        val heart = """{"type":"heart","color":"red"}"""
        val won = """{"puzzles":[{"completed":true}],"eventHistory":[${List(6) { orb }.joinToString(",")}]}"""
        val finished = """{"puzzles":[{"completed":true},{"completed":true},{"completed":true},{"completed":true}],"eventHistory":[$orb,$orb,$heart]}"""
        val going = """{"puzzles":[{"completed":true}],"eventHistory":[$orb]}"""

        assertEquals(read(PuzzleResult(solved = true, score = 6)), parseGameState(orbs, storage("profile-1:daily-orbs-$today", won), today))
        assertEquals(read(PuzzleResult(solved = false, score = 2)), parseGameState(orbs, storage("daily-orbs-$today", finished), today))
        assertEquals(read(null), parseGameState(orbs, storage("daily-orbs-$today", going), today))
        assertEquals(read(null), parseGameState(orbs, storage("daily-orbs-2026-10-26", won), today))
    }

    @Test
    fun aCatfishingGameIsFoundByItsDayNumberAndScoredOutOfTen() {
        val completed = storage("game-856", """{"s":1,"c":[7,1]}""")
        val going = storage("game-856", """{"s":0,"a":[2,2],"u":0}""")
        val yesterday = storage("game-855", """{"s":1,"c":[7,1]}""")

        assertEquals(read(PuzzleResult(score = 7)), parseGameState(catfishing, completed, today))
        assertEquals(read(null), parseGameState(catfishing, going, today))
        assertEquals(read(null), parseGameState(catfishing, yesterday, today))
    }

    @Test
    fun storageTheReaderCannotUnderstandIsUnrecognisedNotGuessedAt() {
        assertEquals(GameReading.Unrecognised, parseGameState(wordle, storage("games-state-wordleV2/ANON", """{"something":"else"}"""), today))
        assertEquals(GameReading.Unrecognised, parseGameState(catfishing, storage("game-856", "[]"), today))
        assertEquals(GameReading.Unrecognised, parseGameState(wordle, "not json", today))
    }

    @Test
    fun aPageWithNoGameStorageHasSimplyNotBeenPlayed() {
        assertEquals(GameReading.NoStorage, parseGameState(wordle, "{}", today))
    }

    @Test
    fun theReadingScriptSendsOnlyKeysThatBelongToTheGame() {
        val script = orbs.readingScript()

        assertTrue(script.contains("new RegExp(\"daily-orbs-\\\\d{4}"))
        assertFalse(script.contains("document.cookie"))
    }

    @Test
    fun theStatusSaysWhatHappenedInWords() {
        assertEquals("Not played", puzzleStatus(wordle, null))
        assertEquals("Solved in 4 guesses", puzzleStatus(wordle, PuzzleResult(solved = true, score = 4)))
        assertEquals("Solved in 1 guess", puzzleStatus(wordle, PuzzleResult(solved = true, score = 1)))
        assertEquals("Not solved", puzzleStatus(wordle, PuzzleResult(solved = false)))
        assertEquals("Solved, 1 mistake", puzzleStatus(connections, PuzzleResult(solved = true, score = 1)))
        assertEquals("Solved, no mistakes", puzzleStatus(connections, PuzzleResult(solved = true, score = 0)))
        assertEquals("Not solved, 4 mistakes", puzzleStatus(connections, PuzzleResult(solved = false, score = 4)))
        assertEquals("Played, 7 of 10", puzzleStatus(catfishing, PuzzleResult(score = 7)))
        assertEquals("Played", puzzleStatus(catfishing, PuzzleResult()))
        assertEquals("Solved, 6 orbs", puzzleStatus(orbs, PuzzleResult(solved = true, score = 6)))
        assertEquals("Not solved, 2 orbs", puzzleStatus(orbs, PuzzleResult(solved = false, score = 2)))
        assertEquals("Played, 87 points", puzzleStatus(enclose, PuzzleResult(score = 87)))
    }

    @Test
    fun encloseHorseIsScoredFromTodaysDailyLevelInTheSubmissions() {
        val levels = JsonPrimitive("""[{"id":"KHDhIm","date":"2026-10-28"},{"id":"Td9iIW","date":"2026-10-27","bonusId":"lbz7Z1"}]""")
        val submissions = JsonPrimitive("""{"Td9iIW":{"walls":[1,2],"score":87},"old123":{"walls":[],"score":5}}""")
        val oldSubmissions = JsonPrimitive("""{"old123":{"walls":[],"score":5}}""")
        val notAnObject = JsonPrimitive("[]")

        assertEquals(read(PuzzleResult(score = 87)), parseGameState(enclose, """{"enclose_submissions": $submissions, "__daily_levels__": $levels}""", today))
        assertEquals(read(null), parseGameState(enclose, """{"enclose_submissions": $oldSubmissions, "__daily_levels__": $levels}""", today))
        assertEquals(GameReading.NoStorage, parseGameState(enclose, """{"__daily_levels__": $levels}""", today))
        assertEquals(GameReading.Unrecognised, parseGameState(enclose, """{"enclose_submissions": $notAnObject, "__daily_levels__": $levels}""", today))
    }

    @Test
    fun theEncloseHorseReadingScriptAlsoAsksThePageForItsDailyLevels() {
        assertTrue(enclose.readingScript().contains("window.__DAILY_LEVELS__"))
        assertFalse(orbs.readingScript().contains("__DAILY_LEVELS__"))
    }

    @Test
    fun aCalendarDayIsDescribedForAssistiveTechnologyInTheGamesOwnTerms() {
        val month = CalendarMonth(2026, 10)
        val history = listOf(
            PuzzleDay("2026-10-25", PuzzleResult(solved = true, score = 4, source = "manual"), "2026-10-25T10:00:00Z"),
            PuzzleDay("2026-10-14", PuzzleResult(solved = false, source = "native"), "2026-10-14T10:00:00Z"),
        )
        val cells = month.cells(today, history).filterNotNull()
        val orbsDay = PuzzleDay("2026-10-25", PuzzleResult(solved = true, score = 6, source = "manual"), "")

        assertEquals("25 October: solved in 4 guesses, entered by hand", wordle.calendarDescription(cells.first { it.day == 25 }))
        assertEquals("14 October: not solved", wordle.calendarDescription(cells.first { it.day == 14 }))
        assertEquals("1 October: not played", wordle.calendarDescription(cells.first { it.day == 1 }))
        assertEquals("25 October: 6 orbs, solved, entered by hand", orbs.calendarDescription(cells.first { it.day == 25 }.copy(entry = orbsDay)))
    }

    @Test
    fun aMonthStartsOnTheRightWeekdayAndMarksTodayAndTheFuture() {
        val cells = CalendarMonth(2026, 10).cells(today, emptyList())

        assertEquals(3, cells.takeWhile { it == null }.size)
        assertEquals(31, cells.filterNotNull().size)
        assertTrue(cells.filterNotNull().first { it.day == 27 }.today)
        assertTrue(cells.filterNotNull().first { it.day == 28 }.future)
        assertFalse(cells.filterNotNull().first { it.day == 26 }.future)
    }

    @Test
    fun monthsStepAcrossYearsAndFebruaryKnowsLeapYears() {
        assertEquals(CalendarMonth(2025, 12), CalendarMonth(2026, 1).shifted(-1))
        assertEquals(CalendarMonth(2027, 1), CalendarMonth(2026, 12).shifted(1))
        assertEquals(29, CalendarMonth(2028, 2).length)
        assertEquals(28, CalendarMonth(2026, 2).length)
        assertFalse(CalendarMonth(2026, 10).canStepForward(today))
        assertTrue(CalendarMonth(2026, 9).canStepForward(today))
    }

    @Test
    fun theCalendarCannotStepBackPastTheEarliestRecord() {
        val history = listOf(PuzzleDay("2026-09-12", PuzzleResult(solved = true), ""))

        assertTrue(CalendarMonth(2026, 10).canStepBack(history))
        assertFalse(CalendarMonth(2026, 9).canStepBack(history))
        assertFalse(CalendarMonth(2026, 10).canStepBack(emptyList()))
    }

    @Test
    fun timesReadAndShowAsMinutesAndSeconds() {
        assertEquals(270, parseDuration("4:30"))
        assertEquals(5, parseDuration("0:05"))
        assertEquals(null, parseDuration("4:75"))
        assertEquals(null, parseDuration("430"))
        assertEquals("3 min 40 s", secondsText(220))
        assertEquals("6 min 05 s", secondsText(365))
        assertEquals("40 s", secondsText(40))
    }

    @Test
    fun aHandEnteredResultIsReadInThePuzzlesOwnTerms() {
        assertEquals(RecordInput.Valid(PuzzleResult(solved = true, score = 4, seconds = 270)), wordle.recordInput("4", "4:30"))
        assertEquals(RecordInput.Valid(PuzzleResult(solved = false)), wordle.recordInput("x", ""))
        assertEquals(RecordInput.Valid(PuzzleResult(solved = true, score = 1)), connections.recordInput("1", ""))
        assertEquals(RecordInput.Valid(PuzzleResult(solved = false, score = 4)), connections.recordInput("4", ""))
        assertEquals(RecordInput.Valid(PuzzleResult(solved = true, score = 6)), orbs.recordInput("6", ""))
        assertEquals(RecordInput.Valid(PuzzleResult(solved = false, score = 5)), orbs.recordInput("5", ""))
        assertEquals(RecordInput.Valid(PuzzleResult(score = 7)), catfishing.recordInput("7", ""))
        assertEquals(RecordInput.Valid(PuzzleResult(score = 87)), enclose.recordInput("87", ""))
    }

    @Test
    fun aHandEnteredResultThatMakesNoSenseSaysHowToFixIt() {
        assertEquals(RecordInput.Invalid("Enter the number of orbs you collected", null), orbs.recordInput("", ""))
        assertEquals(RecordInput.Invalid("Enter the number of guesses, from 1 to 6, or X", null), wordle.recordInput("7", ""))
        assertEquals(RecordInput.Invalid("Enter the number of mistakes, from 0 to 4", null), connections.recordInput("5", ""))
        assertEquals(RecordInput.Invalid("Enter how many you got right, from 0 to 10", null), catfishing.recordInput("11", ""))
        assertEquals(RecordInput.Invalid(null, "Enter the time like 4:30"), orbs.recordInput("6", "soon"))
        assertEquals(RecordInput.Invalid("Enter the points the game gave you", "Enter the time like 4:30"), enclose.recordInput("-1", "x"))
    }

    @Test
    fun theOverviewCountsPuzzlesPlayedTodayAndNamesTheOnesLeft() {
        val summaries = Puzzles.map { PuzzleSummary(it.id, today, if (it.id == "daily-orbs") null else PuzzleResult(solved = true)) }

        assertEquals(4 to listOf("Daily Orbs"), puzzlesToday(summaries))
        assertEquals(0 to Puzzles.map { it.name }, puzzlesToday(emptyList()))
    }

    @Test
    fun theKeyboardRemembersTheBestMarkForEachLetter() {
        val guesses = listOf(
            WordleGuess("crane", listOf("absent", "present", "absent", "absent", "correct")),
            WordleGuess("horse", listOf("absent", "correct", "correct", "correct", "correct")),
        )

        assertEquals("correct", wordleKeyMarks(guesses)['r'])
        assertEquals("correct", wordleKeyMarks(guesses)['e'])
        assertEquals("absent", wordleKeyMarks(guesses)['c'])
        assertEquals(null, wordleKeyMarks(guesses)['z'])
    }

    @Test
    fun aGuessIsDescribedLetterByLetterForAssistiveTechnology() {
        val guess = WordleGuess("crane", listOf("absent", "present", "absent", "absent", "correct"))

        assertEquals("Guess 1, crane: c not in the word; r in the word, wrong place; a not in the word; n not in the word; e in the right place", wordleGuessDescription(0, guess))
        assertEquals("Board. No guesses yet", wordleBoardDescription(emptyList()))
    }

    @Test
    fun aGameIsReadOnlyOnItsOwnSite() {
        assertTrue(wordle.isOnThisSite("https://www.nytimes.com/games/wordle/index.html"))
        assertFalse(wordle.isOnThisSite("https://www.nytimes.com/games/connections"))
        assertFalse(wordle.isOnThisSite("https://evil.example/nytimes.com/games/wordle"))
    }

    @Test
    fun aSolutionDiffIsNumberedFromItsHunkHeader() {
        val lines = solutionLines("@@ -3,2 +3,3 @@\n keep\n-old\n+new\n+more")

        assertEquals(DiffLine(DiffLine.Kind.Context, 3, 3, "keep"), lines[1])
        assertEquals(DiffLine(DiffLine.Kind.Removed, 4, null, "old"), lines[2])
        assertEquals(DiffLine(DiffLine.Kind.Added, null, 4, "new"), lines[3])
        assertEquals(DiffLine(DiffLine.Kind.Added, null, 5, "more"), lines[4])
    }
}
