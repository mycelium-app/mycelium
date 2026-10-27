package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.WellnessScreen
import org.junit.Rule

private class PuzzleReadingPlatform : Platform by OpeningPlatform() {
    override val readsPages = true
}

private val KeyWords = listOf("Piano", "Lock", "Map", "Typewriter")
private val OtherWords = listOf("Bass", "Mole", "Bench", "Pike", "Stool", "Sole", "Spy", "Throne", "Plant", "Perch", "Sofa", "Agent")

private class PuzzleServer {
    val seen = java.util.Collections.synchronizedList(mutableListOf<Triple<String, String, String>>())
    var failing = false
    private val words = mutableListOf<String>()
    private var mistakes = 0
    private var keysSolved = false

    fun sent(method: String, suffix: String) = seen.toList().lastOrNull { it.first == method && it.second.endsWith(suffix) }

    private fun wordleJson(): String {
        val guesses = words.joinToString(",") { word ->
            val marks = List(5) { if (word == "worse") "correct" else "absent" }.joinToString(",") { "\"$it\"" }

            """{"word":"$word","marks":[$marks]}"""
        }
        val won = words.lastOrNull() == "worse"
        val status = if (won) "won" else "playing"
        val ending = if (won) ",\"answer\":\"worse\",\"seconds\":192" else ""

        return """{"day":"2026-10-27","status":"$status","guesses":[$guesses]$ending}"""
    }

    private fun connectionsJson(): String {
        val remaining = (if (keysSolved) emptyList() else KeyWords) + OtherWords
        val solved = if (keysSolved) """[{"title":"Things with keys","words":["Piano","Lock","Map","Typewriter"],"level":0}]""" else "[]"
        val list = remaining.joinToString(",") { "\"$it\"" }

        return """{"day":"2026-10-27","status":"playing","words":[$list],"solved":$solved,"unsolved":[],"mistakes":$mistakes,"mistakes_allowed":4}"""
    }

    fun api(): MyceliumApi {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = String(request.body.toByteArray())
            seen += Triple(request.method.value, path, body)

            when {
                path.endsWith("/puzzles/wordle/game") || path.endsWith("/puzzles/connections/game") ->
                    if (failing) respond("""{"error":"puzzle_unavailable","message":"The New York Times did not send today's puzzle."}""", HttpStatusCode.BadGateway, json)
                    else respond(if (path.contains("wordle")) wordleJson() else connectionsJson(), HttpStatusCode.OK, json)
                path.endsWith("/puzzles/wordle/guesses") -> {
                    val word = Regex("\"word\":\"([a-z]+)\"").find(body)!!.groupValues[1]

                    if (word == "zzzzz") {
                        respond("""{"error":"not_in_word_list","message":"Not in the word list"}""", HttpStatusCode.BadRequest, json)
                    } else {
                        words += word
                        respond(wordleJson(), HttpStatusCode.OK, json)
                    }
                }
                path.endsWith("/puzzles/connections/guesses") -> {
                    val picked = Regex("\"([A-Za-z]+)\"").findAll(body.substringAfter("[")).map { it.groupValues[1] }.toSet()
                    val outcome = if (picked == KeyWords.toSet()) {
                        keysSolved = true
                        "correct"
                    } else {
                        mistakes += 1
                        if (picked.intersect(KeyWords.toSet()).size == 3) "one_away" else "incorrect"
                    }

                    respond("""{"outcome":"$outcome","game":${connectionsJson()}}""", HttpStatusCode.OK, json)
                }
                path.endsWith("/wellness/puzzles") -> respond(WellnessPuzzlesJson, HttpStatusCode.OK, json)
                path.endsWith("/wellness/settings") -> respond(WellnessSettingsJson, HttpStatusCode.OK, json)
                path.endsWith("/wellness/overview") -> respond(WellnessOverviewJson, HttpStatusCode.OK, json)
                request.method != HttpMethod.Get -> respond("", HttpStatusCode.NoContent)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }

        return MyceliumApi("https://mycelium.test", "token", engine)
    }
}

class PuzzleGamesTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val server = PuzzleServer()
    private val phone = OpeningPlatform()

    private fun show(platform: Platform = phone) {
        val store = Store(server.api(), scopes.create())

        runBlocking { store.wellness.load() }
        rule.setContent { MyceliumTheme(true) { WellnessScreen(store, platform, startOn = "puzzles") } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open Wordle").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun open(puzzle: String) {
        show()
        rule.onNodeWithContentDescription("Open $puzzle").performClick()
    }

    private fun press(vararg keys: String) {
        for (key in keys) {
            rule.onNodeWithContentDescription(key).performClick()
        }
    }

    @Test
    fun aWinningWordleGuessEndsTheGameWithTheTimeAndStreak() {
        open("Wordle")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Keyboard").fetchSemanticsNodes().isNotEmpty() }

        press("W", "O", "R", "S", "E", "Enter")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Solved in 1 guess").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"word":"worse"}""", server.sent("POST", "/puzzles/wordle/guesses")!!.third)
        rule.onNodeWithText("3 min 12 s").assertIsDisplayed()
        rule.onNodeWithText("12 days").assertIsDisplayed()
        rule.onNodeWithText("Back to puzzles").assertIsDisplayed()
    }

    @Test
    fun aWordMissingFromTheListIsExplainedAndAnIncompleteWordIsNotSent() {
        open("Wordle")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Keyboard").fetchSemanticsNodes().isNotEmpty() }

        press("W", "O", "Enter")
        rule.onNodeWithText("Enter five letters").assertIsDisplayed()
        assertNull(server.sent("POST", "/puzzles/wordle/guesses"))

        press("Delete", "Delete", "Z", "Z", "Z", "Z", "Z", "Enter")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Not in the word list").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aFinishedGuessIsAnnouncedLetterByLetterAndColouredKeysSayWhatTheyAre() {
        open("Wordle")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Keyboard").fetchSemanticsNodes().isNotEmpty() }

        press("C", "R", "A", "N", "E", "Enter")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Guess 1, crane", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Board. Guess 1, crane", substring = true).assertExists()
        rule.onNodeWithContentDescription("C, not in the word").assertExists()
    }

    @Test
    fun connectionsWordsAreToggleButtonsAndAGroupNeedsExactlyFour() {
        open("Connections")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Submit").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Pike").performClick()
        rule.onNodeWithText("Pike").assertIsOn()
        rule.onNodeWithText("Bass").assertIsOff()

        rule.onNodeWithText("Submit").performClick()
        rule.onNodeWithText("Select four words").assertIsDisplayed()
        assertNull(server.sent("POST", "/puzzles/connections/guesses"))

        rule.onNodeWithText("Deselect all").performClick()
        rule.onNodeWithText("Pike").assertIsOff()
    }

    @Test
    fun aCorrectGroupJoinsTheSolvedRowsAndAWrongOneCostsAMistake() {
        open("Connections")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Submit").fetchSemanticsNodes().isNotEmpty() }

        for (word in listOf("Bass", "Mole", "Bench", "Pike")) {
            rule.onNodeWithText(word).performClick()
        }
        rule.onNodeWithText("Submit").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Incorrect").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Mistakes left: 3").assertIsDisplayed()

        for (word in KeyWords) {
            rule.onNodeWithText(word).performClick()
        }
        rule.onNodeWithText("Submit").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Correct").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Solved: Things with keys", substring = true).assertExists()
    }

    @Test
    fun aPuzzleThatDoesNotLoadOffersTryAgainAndPlayingOnTheSite() {
        server.failing = true
        open("Wordle")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Could not load today's Wordle").fetchSemanticsNodes().isNotEmpty() }

        server.failing = false
        rule.onNodeWithText("Try again").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Keyboard").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun playingOnTheSiteFromAPhoneOpensTheBrowserAndAsksForTheResult() {
        server.failing = true
        open("Connections")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Could not load today's Connections").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Play on nytimes.com").performClick()

        assertEquals(listOf("https://www.nytimes.com/games/connections"), phone.opened)
        rule.onNodeWithText("Record Connections for today").assertIsDisplayed()
    }

    @Test
    fun recordingAResultSaysWhatIsWrongThenSavesItInThePuzzlesOwnTerms() {
        show()
        rule.onNodeWithContentDescription("Record a result for Daily Orbs").performClick()
        rule.onNodeWithText("Record Daily Orbs for today").assertIsDisplayed()

        rule.onNodeWithText("Save").performClick()
        rule.onAllNodesWithText("Enter the number of orbs you collected")[0].assertIsDisplayed()
        assertNull(server.sent("PUT", "/puzzles/daily-orbs/today"))

        rule.onNodeWithContentDescription("Orbs collected", substring = true).performTextInput("6")
        rule.onNodeWithContentDescription("Time taken (optional)").performTextInput("4:30")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { server.sent("PUT", "/puzzles/daily-orbs/today") != null }
        val body = server.sent("PUT", "/puzzles/daily-orbs/today")!!.third
        assertTrue(body.contains("\"score\":6") && body.contains("\"seconds\":270") && body.contains("\"solved\":true") && body.contains("\"source\":\"manual\""), body)
    }

    @Test
    fun desktopShowsTheStreakAndAMonthCalendarWithEachDaysScoreForTheChosenPuzzle() {
        show(PuzzleReadingPlatform())

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Solved in 4 guesses").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Average time").assertIsDisplayed()
        rule.onNodeWithText("3 min 40 s").assertIsDisplayed()
        rule.onNodeWithContentDescription("25 October: solved in 4 guesses, entered by hand").assertExists()
        rule.onNodeWithContentDescription("14 October: not solved").assertExists()
        rule.onNodeWithContentDescription("1 October: not played").assertExists()

        rule.onNodeWithContentDescription("Previous month").performClick()
        rule.onNodeWithText("September 2026").assertIsDisplayed()
        rule.onNodeWithContentDescription("12 September: solved in 5 guesses").assertExists()
    }

    @Test
    fun desktopOffersToRecordAResultOnlyWhenTheGameCouldNotBeRead() {
        val store = Store(server.api(), scopes.create())

        runBlocking { store.wellness.load() }
        store.wellness.markUnreadable("daily-orbs")
        rule.setContent { MyceliumTheme(true) { WellnessScreen(store, PuzzleReadingPlatform(), startOn = "puzzles") } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open Wordle").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(0, rule.onAllNodesWithText("Record a result").fetchSemanticsNodes().size)

        rule.onNodeWithText("Daily Orbs").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Record a result").fetchSemanticsNodes().isNotEmpty() }
    }
}
