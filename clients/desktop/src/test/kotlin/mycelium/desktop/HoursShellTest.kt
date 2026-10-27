package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import kotlinx.coroutines.runBlocking
import mycelium.api.Hours
import mycelium.api.HoursDay
import mycelium.api.Me
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class HoursShellTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val work = hoursGroupJson("g-work", "work", closedAllWeek())
    private val personal = hoursGroupJson("g-personal", "personal")
    private val conversations = listOf(hoursConversationJson("c1", "Work chat", "g-work", "working"), hoursConversationJson("c2", "Personal chat", "g-personal", "working"))

    private fun show(server: HoursServer, selected: String = "all", route: Route = Route.Empty): Store {
        val platform = HoursPlatform(selected)
        val store = server.store(scopes.create(), platform)

        rule.setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route) } }

        return store
    }

    private fun showWhileWorkCloses(): HoursServer {
        val server = HoursServer(listOf(hoursGroupJson("g-work", "work"), personal), conversations)

        server.savedHoursResponse = work

        val store = show(server, selected = "g-work")

        rule.waitForIdle()
        runBlocking { store.saveGroupHours("g-work", Hours(alwaysOpen = false, days = (0..6).map { HoursDay(it, closed = true, opens = "09:00", closes = "18:00") })) }

        return server
    }

    private fun shown(text: String): Int {
        rule.waitForIdle()

        return rule.onAllNodesWithText(text, substring = true).fetchSemanticsNodes().size
    }

    private fun waitFor(text: String) {
        rule.waitUntil(5_000) { rule.onAllNodesWithText(text, substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aClosedGroupsConversationsLeaveTheSidebarWhileOpenOnesStay() {
        show(HoursServer(listOf(work, personal), conversations))

        waitFor("Personal chat")

        assertEquals(0, shown("Work chat"))
    }

    @Test
    fun aClosedGroupsFoldersLeaveTheProjectsList() {
        val folders = listOf(hoursFolderJson("p-work", "work-notes", "g-work"), hoursFolderJson("p-home", "home-notes", "g-personal"))

        show(HoursServer(listOf(work, personal), conversations, projects = folders), route = Route.Projects)

        waitFor("home-notes")

        assertEquals(0, shown("work-notes"))
    }

    @Test
    fun theGroupSelectorOffersToOpenAClosedGroupInsteadOfListingIt() {
        show(HoursServer(listOf(work, personal), conversations))

        waitFor("Personal chat")
        rule.onNodeWithContentDescription("Group: All groups").performClick()

        rule.onNodeWithText("Open a closed group").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("work").fetchSemanticsNodes().size)
    }

    @Test
    fun whenTheSelectedGroupClosesTheClosedScreenShowsWithTheOpenGroupsToGoTo() {
        showWhileWorkCloses()

        waitFor("Open now")

        rule.onNodeWithText("Work is closed").assertIsDisplayed()
        rule.onNodeWithText("Personal").assertIsDisplayed()
        rule.onNodeWithText("Open Work for 15 minutes").assertIsDisplayed()
    }

    @Test
    fun choosingAnOpenGroupOnTheClosedScreenLeavesIt() {
        showWhileWorkCloses()

        waitFor("Open now")
        rule.onNodeWithText("Personal").performClick()
        waitFor("Personal chat")

        assertEquals(0, shown("Open now"))
    }

    @Test
    fun openingOutOfHoursAsksWhyBeforeSendingAnything() {
        val server = showWhileWorkCloses()

        waitFor("Open now")
        rule.onNodeWithText("Open Work for 15 minutes").performClick()
        waitFor("Why do you need it now?")
        rule.onNodeWithText("Open for 15 minutes").performClick()

        rule.onNodeWithText("Enter why you need to open Work").assertIsDisplayed()
        assertNull(server.sent("POST", "/open"))
    }

    @Test
    fun aReasonOpensTheGroupForFifteenMinutesAndTheClosedScreenGoes() {
        val server = showWhileWorkCloses()

        waitFor("Open now")
        rule.onNodeWithText("Open Work for 15 minutes").performClick()
        waitFor("Why do you need it now?")
        rule.onNodeWithContentDescription("Why do you need it now?").performTextInput("Deploy is failing")
        rule.onNodeWithText("Open for 15 minutes").performClick()
        rule.waitUntil(5_000) { server.sent("POST", "/groups/g-work/open") != null }

        assertTrue(server.sent("POST", "/groups/g-work/open")!!.third.contains("\"reason\":\"Deploy is failing\""))
    }

    @Test
    fun whileMyceliumIsClosedEverythingIsClosedAndOnlyWellnessAndTheFooterStay() {
        val closedSettings = WellnessSettingsJson.replace(Regex("\"closed\":false"), "\"closed\":true")
        val server = HoursServer(listOf(work, personal), conversations, closedSettings)

        show(server)
        waitFor("Mycelium is closed")

        rule.onNodeWithText("Open a group for 15 minutes").assertIsDisplayed()
        rule.onNodeWithText("Wellness & Learning").assertExists()
        assertEquals(0, shown("New conversation"))
        rule.onNodeWithContentDescription("Settings").assertExists()
    }

    @Test
    fun aCountdownShowsInTheLastFifteenMinutesBeforeAGroupClosesAndTheComposerIsUntouched() {
        val closing = """{"always_open":false,"days":${hoursDaysJson(closed = false, opens = clockIn(-5), closes = clockIn(12))}}"""
        val server = HoursServer(listOf(hoursGroupJson("g-work", "work", closing), personal), conversations)

        show(server, selected = "g-work")

        rule.waitUntil(5_000) { rule.onAllNodes(hasContentDescription("minutes left", substring = true)).fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(hasContentDescription("minutes left", substring = true)).assertIsDisplayed()
        rule.onNodeWithText("Hours").assertIsDisplayed()
    }

    @Test
    fun anOpeningOutOfHoursSaysWhenItEndsAndCloseNowEndsItAtOnce() {
        val opened = hoursGroupJson("g-work", "work", closedAllWeek(), openedUntil = isoIn(14))
        val server = HoursServer(listOf(opened, personal), conversations)

        show(server, selected = "g-work")

        waitFor("is open out of hours until")
        rule.onNodeWithText("Close now").performClick()
        rule.waitUntil(5_000) { server.sent("DELETE", "/groups/g-work/open") != null }
    }
}
