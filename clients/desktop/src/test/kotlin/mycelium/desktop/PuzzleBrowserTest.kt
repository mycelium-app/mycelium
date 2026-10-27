package mycelium.desktop

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import mycelium.PageReading
import mycelium.Platform
import mycelium.state.Store
import mycelium.state.puzzleOf
import mycelium.ui.MyceliumTheme
import mycelium.ui.PuzzleBrowser
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

private class PageThatReads(private val storage: String, private val url: String) : Platform by OpeningPlatform() {
    @Composable
    override fun WebContent(url: String, token: String?, modifier: Modifier, reading: PageReading?) {
        LaunchedEffect(reading) {
            if (reading != null && reading.allowedUrl(this@PageThatReads.url)) {
                reading.onRead(storage)
            }
        }
    }
}

private fun nytWordle(printDate: String, status: String, rows: String) =
    """{"games-state-wordleV2/ANON":"{\"states\":[{\"puzzleId\":1,\"printDate\":\"$printDate\",\"data\":{\"status\":\"$status\",\"boardState\":$rows}}]}"}"""

private fun nytConnections(printDate: String, won: Boolean, mistakes: Int) =
    """{"games-state-connections/ANON":"{\"states\":[{\"puzzleId\":1,\"printDate\":\"$printDate\",\"data\":{\"puzzleComplete\":true,\"puzzleWon\":$won,\"mistakes\":$mistakes}}]}"}"""

class PuzzleBrowserTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = WellnessRequests()
    private var wentBack = false

    private fun show(storage: String, url: String = "https://www.nytimes.com/games/connections", puzzle: String = "connections"): Store {
        val store = Store(requests.api(), scopes.create())

        rule.setContent { MyceliumTheme(true) { PuzzleBrowser(store, PageThatReads(storage, url), puzzleOf(puzzle)!!) { wentBack = true } } }

        return store
    }

    @Test
    fun aFinishedWordleReadFromThePageIsRecordedAsReadFromTheGame() {
        show(nytWordle("2026-10-27", "WIN", """[\"crane\",\"slate\",\"stale\",\"\",\"\",\"\"]"""), "https://www.nytimes.com/games/wordle/index.html", "wordle")

        rule.waitUntil(5_000) { requests.sent("PUT", "/wellness/puzzles/wordle/today") != null }

        val body = requests.sent("PUT", "/wellness/puzzles/wordle/today")!!.third

        assertTrue(body.contains("\"guesses\":3") && body.contains("\"solved\":true") && body.contains("\"source\":\"read\""), body)
        rule.onNodeWithText("Solved in 3 guesses, recorded").assertIsDisplayed()
    }

    @Test
    fun aGameFromAnotherDayIsNotRecordedAsToday() {
        show(nytConnections("2026-10-26", won = true, mistakes = 1))

        rule.waitUntil(5_000) { requests.sent("GET", "/wellness/puzzles") != null }
        rule.waitForIdle()

        assertNull(requests.sent("PUT", "/wellness/puzzles/connections/today"))
        rule.onNodeWithText("Waiting for the game").assertIsDisplayed()
    }

    @Test
    fun storageTheReaderCannotUnderstandIsReportedAsUnreadable() {
        show("""{"games-state-connections/ANON":"{\"unexpected\":true}"}""")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Could not read the game").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun leavingBeforePlayingDoesNotLabelTheGameUnreadable() {
        val store = show("{}")

        rule.waitUntil(5_000) { requests.sent("GET", "/wellness/puzzles") != null }
        rule.onNodeWithText("Back to puzzles").performClick()

        assertTrue(wentBack)
        assertEquals(emptySet(), store.wellness.state.value.unreadable)
    }

    @Test
    fun anotherSiteIsNeverRead() {
        show(nytConnections("2026-10-27", won = true, mistakes = 0), url = "https://evil.example/games/connections")

        rule.waitUntil(5_000) { requests.sent("GET", "/wellness/puzzles") != null }
        rule.waitForIdle()

        assertNull(requests.sent("PUT", "/wellness/puzzles/connections/today"))
    }
}
