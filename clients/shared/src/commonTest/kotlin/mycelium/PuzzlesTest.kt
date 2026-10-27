package mycelium

import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.PuzzleResult
import mycelium.state.DiffLine
import mycelium.state.GameReading
import mycelium.state.isOnThisSite
import mycelium.state.parseGameState
import mycelium.state.puzzleOf
import mycelium.state.puzzleStatus
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
    private val today = "2026-10-27"

    private fun storage(key: String, value: String): String = """{"$key": ${JsonPrimitive(value)}}"""

    private fun nyt(game: String, printDate: String, data: String, extra: String = ""): String =
        storage("games-state-$game/ANON", """{"states":[{"puzzleId":1,"printDate":"$printDate","schemaVersion":"1.0","timestamp":1,"data":$data$extra}]}""")

    private fun read(result: PuzzleResult?) = GameReading.Read(result)

    @Test
    fun aWonWordleIsReadAsSolvedInTheRowsFilled() {
        val data = """{"boardState":["crane","slate","stale","","",""],"currentRowIndex":3,"status":"WIN","isPlayingArchive":false}"""

        assertEquals(read(PuzzleResult(solved = true, guesses = 3)), parseGameState(wordle, nyt("wordleV2", today, data), today))
    }

    @Test
    fun aLostWordleIsNotSolved() {
        val data = """{"boardState":["a","b","c","d","e","f"],"status":"FAIL"}"""

        assertEquals(read(PuzzleResult(solved = false, guesses = 6)), parseGameState(wordle, nyt("wordleV2", today, data), today))
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

        assertEquals(read(PuzzleResult(solved = true, guesses = 2)), parseGameState(wordle, signedIn, today))
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

        assertEquals(read(PuzzleResult(solved = true, mistakes = 1)), parseGameState(connections, nyt("connections", today, won), today))
        assertEquals(read(PuzzleResult(solved = false, mistakes = 4)), parseGameState(connections, nyt("connections", today, lost), today))
        assertEquals(read(null), parseGameState(connections, nyt("connections", today, going), today))
    }

    @Test
    fun aDailyOrbsDayIsWonAtSixOrbsOrLostWhenAllFourPuzzlesAreDone() {
        val orb = """{"type":"orb","color":"red"}"""
        val heart = """{"type":"heart","color":"red"}"""
        val won = """{"puzzles":[{"completed":true}],"eventHistory":[${List(6) { orb }.joinToString(",")}]}"""
        val finished = """{"puzzles":[{"completed":true},{"completed":true},{"completed":true},{"completed":true}],"eventHistory":[$orb,$orb,$heart]}"""
        val going = """{"puzzles":[{"completed":true}],"eventHistory":[$orb]}"""

        assertEquals(read(PuzzleResult(solved = true, score = "6 orbs")), parseGameState(orbs, storage("profile-1:daily-orbs-$today", won), today))
        assertEquals(read(PuzzleResult(solved = false, score = "2 orbs")), parseGameState(orbs, storage("daily-orbs-$today", finished), today))
        assertEquals(read(null), parseGameState(orbs, storage("daily-orbs-$today", going), today))
        assertEquals(read(null), parseGameState(orbs, storage("daily-orbs-2026-10-26", won), today))
    }

    @Test
    fun aCatfishingGameIsFoundByItsDayNumberAndScoredOutOfTen() {
        val completed = storage("game-856", """{"s":1,"c":[7,1]}""")
        val going = storage("game-856", """{"s":0,"a":[2,2],"u":0}""")
        val yesterday = storage("game-855", """{"s":1,"c":[7,1]}""")

        assertEquals(read(PuzzleResult(score = "7 of 10")), parseGameState(catfishing, completed, today))
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
        assertEquals("Not played", puzzleStatus(null))
        assertEquals("Solved in 4 guesses", puzzleStatus(PuzzleResult(solved = true, guesses = 4)))
        assertEquals("Solved in 1 guess", puzzleStatus(PuzzleResult(solved = true, guesses = 1)))
        assertEquals("Solved, 1 mistake", puzzleStatus(PuzzleResult(solved = true, mistakes = 1)))
        assertEquals("Played, 7 of 10", puzzleStatus(PuzzleResult(score = "7 of 10")))
        assertEquals("Played", puzzleStatus(PuzzleResult()))
        assertEquals("Solved, 6 orbs", puzzleStatus(PuzzleResult(solved = true, score = "6 orbs")))
        assertEquals("Not solved, 2 orbs", puzzleStatus(PuzzleResult(solved = false, score = "2 orbs")))
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
