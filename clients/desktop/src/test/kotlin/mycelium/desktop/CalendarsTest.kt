package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import mycelium.api.ChatToolServer
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.Store
import mycelium.ui.AccountPage
import mycelium.ui.IntegrationsCatalogue
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.PageActions
import mycelium.ui.SetupFlow
import mycelium.ui.integrationsOf
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Found = """{"username":"sam","calendars":[{"name":"Personal"},{"name":"Work"},{"name":"Family","read_only":true}]}"""

private const val CalendarAccount = """{"id":"c1","name":"Calendar","kind":"caldav","url":"https://cal.example.com","account":"sam","settings":{"principal":"https://cal.example.com/dav/principals/sam/","calendar_home":"https://cal.example.com/dav/calendars/sam/"},"token_hint":"••••7k2q","enabled":true,"tools":[{"name":"list_events","description":"List","kind":"read"},{"name":"create_event","description":"Create","kind":"write","asks":true}],"created_at":"2026-10-27T10:00:00Z"}"""

private const val Listed = """[{"name":"Personal"},{"name":"Work"},{"name":"Family","read_only":true}]"""

class CalendarsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Pair<String, String>>()
    private var testAnswer = HttpStatusCode.OK to Found
    private var finished = false

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += path to String(request.body.toByteArray())

            when {
                path.endsWith("/caldav-test") -> respond(testAnswer.second, testAnswer.first, json)
                path.endsWith("/calendars") -> respond(Listed, HttpStatusCode.OK, json)
                path.endsWith("/api-access") -> respond("[]", HttpStatusCode.OK, json)
                request.method.value == "POST" -> respond(CalendarAccount, HttpStatusCode.Created, json)
                else -> respond("[]", HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    private fun openSetup() {
        val store = store()

        rule.setContent { MyceliumTheme(true) { SetupFlow(store, "caldav", null, emptySet(), {}, { finished = true }, {}, {}) } }
    }

    private fun waitForText(text: String) = rule.waitUntil(5_000) { rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }

    private fun toTestStep(password: String = "app-secret") {
        rule.onNodeWithContentDescription("Server address").performTextInput("https://cal.example.com")
        rule.onNodeWithText("Continue").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("App password").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Username").performTextInput("sam")
        rule.onNodeWithContentDescription("App password").performTextInput(password)
        rule.onNodeWithText("Continue").performClick()
    }

    @Test
    fun theTestStepListsTheFoundCalendarsMarksAReadOnlyOneAndSendsTheCredential() {
        openSetup()

        toTestStep()

        waitForText("Connected as sam. Found 3 calendars.")
        rule.onNodeWithText("Personal").assertExists()
        rule.onNodeWithText("Family, read only").assertExists()
        assertTrue(requests.any { it.first.endsWith("/caldav-test") && it.second.contains("app-secret") && it.second.contains("\"username\":\"sam\"") })
    }

    @Test
    fun connectingNamesTheAccountCalendarAndSendsKindUsernameAndAppPassword() {
        openSetup()

        toTestStep()
        waitForText("Connected as sam. Found 3 calendars.")
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithContentDescription("Name").assertExists()
        rule.onNodeWithText("Add integration").performClick()

        rule.waitUntil(5_000) { finished }
        val created = requests.last { it.first.endsWith("/chat-tool-servers") }.second

        assertTrue(created.contains("\"name\":\"Calendar\""), created)
        assertTrue(created.contains("\"kind\":\"caldav\""), created)
        assertTrue(created.contains("\"account\":\"sam\""), created)
        assertTrue(created.contains("\"bearer_token\":\"app-secret\""), created)
    }

    @Test
    fun aRejectedAppPasswordOffersBackToCredential() {
        testAnswer = HttpStatusCode.BadRequest to """{"error":"caldav_credential","message":"cal.example.com did not accept the username or app password. Check them, or create a new app password."}"""
        openSetup()

        toTestStep("wrong")

        waitForText("cal.example.com did not accept the username or app password. Check them, or create a new app password.")
        rule.onNodeWithText("Back to credential").performClick()
        rule.onNodeWithContentDescription("Step 2 of 4: Credential").assertExists()
        rule.onNodeWithContentDescription("Username").assertExists()
    }

    @Test
    fun aServerThatIsNotCalDavOffersBackToAddress() {
        testAnswer = HttpStatusCode.BadRequest to """{"error":"caldav_address","message":"cal.example.com does not look like a CalDAV server. Enter the address your calendar provider gives for CalDAV."}"""
        openSetup()

        toTestStep()

        waitForText("cal.example.com does not look like a CalDAV server. Enter the address your calendar provider gives for CalDAV.")
        rule.onNodeWithText("Try again").assertExists()
        rule.onNodeWithText("Back to address").performClick()
        rule.onNodeWithContentDescription("Step 1 of 4: Address").assertExists()
    }

    @Test
    fun theCatalogueOffersACalendarTileUnderACalendarChip() {
        rule.setContent { MyceliumTheme(true) { IntegrationsCatalogue(emptyList(), emptyList(), {}, {}, {}, {}) } }

        rule.onNodeWithText("Calendar · CalDAV").assertExists()
        rule.onAllNodesWithText("Calendar").assertCountEquals(2)
    }

    @Test
    fun theAccountOverviewShowsTheCredentialFactsAndTheCalendarsFetchedLive() {
        val store = store()
        val account = MyceliumJson.decodeFromString(ChatToolServer.serializer(), CalendarAccount)
        val integration = integrationsOf(listOf(account)).single()

        rule.setContent { MyceliumTheme(true) { AccountPage(store, integration, account, PageActions({}, {}, {}, {}, {}, {}, {}), {}, {}) } }

        rule.onNodeWithText("Ends in ••••7k2q").assertExists()
        rule.onNodeWithText("https://cal.example.com/dav/calendars/sam/").assertExists()
        rule.onNodeWithText("Replace app password").assertExists()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Read only").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Family").assertExists()
        rule.onNodeWithText("Remove account").assertExists()
        assertEquals(0, rule.onAllNodesWithText("Replace token").fetchSemanticsNodes().size)
    }
}
