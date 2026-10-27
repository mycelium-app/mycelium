package mycelium

import mycelium.api.PuzzleResult
import mycelium.state.DiffLine
import mycelium.state.isOnThisSite
import mycelium.state.parseGameState
import mycelium.state.puzzleOf
import mycelium.state.puzzleStatus
import mycelium.state.solutionLines
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PuzzlesTest {
    private val wordle = puzzleOf("wordle")!!
    private val today = 1_700_000_000_000L

    private fun storage(state: String): String = """{"nyt-wordle-state": ${kotlinx.serialization.json.JsonPrimitive(state)}}"""

    @Test
    fun aWonWordleIsReadAsSolvedInTheRowsUsed() {
        val state = """{"gameStatus":"WIN","rowIndex":4,"lastPlayedTs":$today}"""

        assertEquals(PuzzleResult(solved = true, guesses = 4), parseGameState(wordle, storage(state)) { it == today })
    }

    @Test
    fun aLostWordleIsNotSolved() {
        val state = """{"gameStatus":"FAIL","rowIndex":6,"lastPlayedTs":$today}"""

        assertEquals(PuzzleResult(solved = false, guesses = 6), parseGameState(wordle, storage(state)) { it == today })
    }

    @Test
    fun aWordleFromAnotherDayIsNotToday() {
        val state = """{"gameStatus":"WIN","rowIndex":4,"lastPlayedTs":$today}"""

        assertNull(parseGameState(wordle, storage(state)) { false })
    }

    @Test
    fun storageTheReaderDoesNotKnowIsNotGuessedAt() {
        assertNull(parseGameState(wordle, """{"nyt-something-else":"{}"}""") { true })
        assertNull(parseGameState(puzzleOf("catfishing")!!, """{"catfishing-state":"{}"}""") { true })
        assertNull(parseGameState(wordle, "not json") { true })
    }

    @Test
    fun theStatusSaysWhatHappenedInWords() {
        assertEquals("Not played", puzzleStatus(null))
        assertEquals("Solved in 4 guesses", puzzleStatus(PuzzleResult(solved = true, guesses = 4)))
        assertEquals("Solved in 1 guess", puzzleStatus(PuzzleResult(solved = true, guesses = 1)))
        assertEquals("Solved, 1 mistake", puzzleStatus(PuzzleResult(solved = true, mistakes = 1)))
        assertEquals("Played, 7 of 10", puzzleStatus(PuzzleResult(score = "7 of 10")))
        assertEquals("Played", puzzleStatus(PuzzleResult()))
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
