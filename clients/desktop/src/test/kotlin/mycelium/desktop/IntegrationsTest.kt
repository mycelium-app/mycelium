package mycelium.desktop

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.filterToOne
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.StreamMessage
import mycelium.state.Store
import mycelium.ui.ChatToolsSettings
import mycelium.ui.LocalOpenConversation
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.time.Clock
import kotlin.time.Duration.Companion.hours

internal const val DocsServer = """{"id":"o1","name":"Docs","kind":"mcp","url":"https://docs.test/mcp","settings":{"app":"outline"},"oauth":true,"enabled":true,"always_allow":false,"status":"expires_soon","token_expires_at":"2027-01-24T00:00:00Z","last_used_at":"2026-10-26T09:12:00Z","tools":[{"name":"search","kind":"read"},{"name":"create_document","kind":"write","asks":true}],"created_at":"2026-10-22T10:00:00Z"}"""

internal const val MailA = """{"id":"e1","name":"Email a","kind":"email","url":"","account":"a@example.com","token_hint":"••••0123","enabled":true,"always_allow":false,"tools":[{"name":"search_messages","kind":"read"},{"name":"send_message","kind":"write","asks":true}],"created_at":"2026-10-22T10:00:00Z"}"""

internal const val MailB = """{"id":"e2","name":"Email b","kind":"email","url":"","account":"b@example.com","token_hint":"••••4567","enabled":true,"always_allow":false,"tools":[{"name":"search_messages","kind":"read"},{"name":"send_message","kind":"write","asks":true}],"created_at":"2026-10-22T11:00:00Z"}"""

internal const val ZulipLocked = """{"id":"z1","name":"Zulip","kind":"zulip","url":"https://chat.test","account":"ada@example.com","token_hint":"••••cdef","enabled":true,"always_allow":false,"locked":true,"tools":[{"name":"get_messages","kind":"read","asks":true},{"name":"send_message","kind":"write","asks":true}],"created_at":"2026-10-22T12:00:00Z"}"""

internal const val TrackerOff = """{"id":"t1","name":"Tracker","kind":"mcp","url":"https://mcp.tracker.test","enabled":false,"always_allow":false,"tools":[{"name":"list_issues","kind":"read"}],"created_at":"2026-10-22T13:00:00Z"}"""

internal const val StoatExpired = """{"id":"s1","name":"Stoat","kind":"stoat","url":"https://api.stoat.chat","token_hint":"••••cdef","enabled":true,"always_allow":false,"status":"expired","token_expires_at":"2026-10-01T00:00:00Z","tools":[{"name":"list_servers","kind":"read"}],"created_at":"2026-10-22T14:00:00Z"}"""

internal const val CalendarServer = """{"id":"c1","name":"Calendar","kind":"caldav","url":"https://cal.example.com","account":"sam","settings":{"principal":"https://cal.example.com/dav/principals/sam/","calendar_home":"https://cal.example.com/dav/calendars/sam/"},"token_hint":"••••7k2q","enabled":true,"always_allow":false,"tools":[{"name":"list_events","kind":"read"},{"name":"create_event","kind":"write","asks":true}],"created_at":"2026-10-22T15:00:00Z"}"""

internal const val AcmeGroup = """{"id":"g1","name":"acme","folders":[],"created_at":"2026-10-22T10:00:00Z"}"""

private fun call(id: String, server: String, serverName: String, tool: String, kind: String, conversation: String?, surface: String, provider: String?, decision: String, review: String?, hoursAgo: Int): String {
    val at = Clock.System.now() - hoursAgo.hours

    return """{"id":"$id","server_id":"$server","server_name":"$serverName","tool":"$tool","kind":"$kind","conversation_id":${conversation?.let { "\"$it\"" }},"surface":"$surface","provider":${provider?.let { "\"$it\"" }},"decision":"$decision","review":${review?.let { "\"$it\"" }},"is_error":false,"at":"$at"}"""
}

private fun conversationJson(id: String, title: String, agent: String) = """{"id":"$id","owner":"u","node_id":"n1","agent":"$agent","cwd":"/work/$id","title":"$title","status":"done","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

@Composable
private fun Scrolling(content: @Composable () -> Unit) {
    Column(Modifier.verticalScroll(rememberScrollState())) { content() }
}

class IntegrationsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<HttpRequestData>()
    private val bodies = CopyOnWriteArrayList<String>()
    private var servers = listOf(DocsServer, MailA, MailB, ZulipLocked, TrackerOff)
    private var calls = emptyList<String>()
    private val opened = CopyOnWriteArrayList<String>()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            requests += request
            bodies += String(request.body.toByteArray())
            val path = request.url.encodedPath

            when {
                path.endsWith("/integration-calls") -> respond("[${calls.filter { call -> request.url.parameters["server"]?.let { call.contains("\"server_id\":\"$it\"") } ?: true }.joinToString(",")}]", HttpStatusCode.OK, json)
                path.endsWith("/groups") && request.method == HttpMethod.Get -> respond("[$AcmeGroup]", HttpStatusCode.OK, json)
                path.endsWith("/forges") -> respond("[]", HttpStatusCode.OK, json)
                request.method == HttpMethod.Patch || request.method == HttpMethod.Put -> respond(servers.first { path.contains("/${Regex("\"id\":\"([^\"]+)\"").find(it)!!.groupValues[1]}") }, HttpStatusCode.OK, json)
                path.endsWith("/chat-tool-servers") -> respond("[${servers.joinToString(",")}]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    private fun show(store: Store = store(), ready: String = "Docs"): Store {
        rule.setContent { MyceliumTheme(true) { CompositionLocalProvider(LocalOpenConversation provides { opened += it }) { Scrolling { ChatToolsSettings(store) } } } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText(ready).fetchSemanticsNodes().isNotEmpty() }

        return store
    }

    private fun openCard(name: String) {
        rule.onAllNodesWithText(name).filterToOne(!hasContentDescription("Category: $name")).performClick()
    }

    @Test
    fun searchFindsAnIntegrationByItsToolName() {
        show()

        rule.onNodeWithContentDescription("Search integrations and accounts").performTextInput("send_message")

        rule.onNodeWithText("Email").assertExists()
        rule.onNodeWithText("Zulip").assertExists()
        rule.onNodeWithText("Tracker").assertDoesNotExist()
    }

    @Test
    fun categoryAndStatusFiltersNarrowTheCards() {
        show()

        rule.onNodeWithContentDescription("Category: Mail").performClick()
        rule.onNodeWithText("Email").assertExists()
        rule.onNodeWithText("Zulip").assertDoesNotExist()

        rule.onNodeWithContentDescription("Category: All").performClick()
        rule.onNodeWithContentDescription("Status: Any status").performClick()
        rule.onAllNodesWithText("Disabled").onLast().performClick()

        rule.onNodeWithText("Tracker").assertExists()
        rule.onNodeWithText("Email").assertDoesNotExist()
    }

    @Test
    fun withNoIntegrationsTheHomeInvitesTheFirstOne() {
        servers = emptyList()
        rule.setContent { MyceliumTheme(true) { Scrolling { ChatToolsSettings(store()) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("No integrations yet").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Add your first integration").performClick()

        rule.onNodeWithText("Built in").assertExists()
    }

    @Test
    fun theCatalogueFindsLastFmBySearchingForScrobble() {
        show()

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithContentDescription("Search services and tools").performTextInput("scrobble")

        rule.onNodeWithText("Last.fm").assertExists()
        rule.onNodeWithText("Zulip").assertDoesNotExist()
        rule.onNodeWithText("Any MCP server").assertDoesNotExist()
    }

    @Test
    fun theCatalogueFindsFreeAgentBySearchingForInvoice() {
        show()

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithContentDescription("Search services and tools").performTextInput("invoice")

        rule.onNodeWithText("FreeAgent").assertExists()
        rule.onNodeWithText("Discord").assertDoesNotExist()
    }

    @Test
    fun choosingAnAddedTileOpensItsPageAndAnUnaddedOneOpensItsSetupDialog() {
        show()

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithText("Stoat").performClick()
        rule.onNodeWithContentDescription("Stoat API address").assertExists()
        rule.onNodeWithText("Cancel").performClick()
        rule.onNodeWithText("Email").performClick()

        rule.onNodeWithText("Accounts").assertExists()
        rule.onNodeWithText("Add account").assertExists()
    }

    @Test
    fun theCalendarTileOpensTheCalendarDialogNotTheGenericServerOne() {
        show()

        rule.onNodeWithText("Add integration").performClick()
        rule.onAllNodesWithText("Calendar").onLast().performClick()

        rule.onNodeWithText("Add a calendar").assertExists()
    }

    @Test
    fun aCalendarAccountHasACardUnderTheCalendarFilterThatOpensItsAccountPage() {
        servers = listOf(DocsServer, CalendarServer)
        show()

        rule.onNodeWithContentDescription("Category: Calendar").performClick()
        rule.onNodeWithText("docs.test", substring = true).assertDoesNotExist()

        openCard("Calendar")
        rule.onNodeWithContentDescription("Open sam").performClick()

        rule.onNodeWithText("https://cal.example.com/dav/calendars/sam/").assertExists()
    }

    @Test
    fun eachAccountHasItsOwnPageAndChangingItsExpiryIsSavedForThatAccount() {
        show()
        openCard("Email")

        rule.onNodeWithContentDescription("Open b@example.com").performClick()
        rule.onNodeWithText("Change expiry").performScrollTo().performClick()
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Patch && it.url.encodedPath.endsWith("/e2") } }
        assertEquals("""{"clear_token_expiry":true}""", bodies.last { it.isNotEmpty() })
    }

    @Test
    fun approvalsAreChangedInADialogForTheChosenAccountOnly() {
        show()
        openCard("Email")

        rule.onNodeWithContentDescription("Open a@example.com").performClick()
        rule.onNodeWithText("Approvals").performClick()
        rule.onNodeWithText("Change approvals").performClick()
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Patch && it.url.encodedPath.endsWith("/e1") } }
        assertTrue(bodies.last { it.isNotEmpty() }.contains("\"policy\""), bodies.last())
    }

    @Test
    fun availabilityCanBeLimitedToChosenGroups() {
        show()
        openCard("Tracker")
        rule.onNodeWithContentDescription("Open mcp.tracker.test").performClick()
        rule.onNodeWithText("Availability").performClick()
        rule.onNodeWithText("Change availability").performClick()
        rule.onNodeWithText("Chosen groups").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Available in acme").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Available in acme").performClick()
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Put && it.url.encodedPath.endsWith("/t1/groups") } }
        assertEquals("""{"groups":["g1"]}""", bodies.last { it.isNotEmpty() })
    }

    @Test
    fun anExpiredAccountNeedsSignInAndItsTokenCanBeReplacedWithoutShowingIt() {
        servers = listOf(StoatExpired)
        show(ready = "Token expired")

        rule.onNodeWithText("Needs attention").assertExists()
        openCard("Stoat")
        rule.onNodeWithContentDescription("Open api.stoat.chat").performClick()

        rule.onNodeWithText("Replace token").performScrollTo().performClick()
        rule.onNodeWithContentDescription("New token").performTextInput("fresh-secret-token")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Patch } }
        assertEquals("""{"clear_token_expiry":true,"bearer_token":"fresh-secret-token"}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("fresh-secret-token").assertCountEquals(0)
    }

    private fun withCalls() {
        calls = listOf(
            call("c1", "e1", "Email a", "search_messages", "read", "k1", "workspace", null, "approved", "shared", 2),
            call("c2", "e1", "Email a", "search_messages", "read", "k1", "workspace", null, "approved", "withheld", 3),
            call("c3", "e1", "Email a", "send_message", "write", "k1", "workspace", null, "declined", null, 4),
            call("c4", "e2", "Email b", "search_messages", "read", "k2", "chat", "OpenRouter", "approved", "removed", 5),
            call("c5", "z1", "Zulip", "get_messages", "read", "k2", "chat", "OpenRouter", "approved", "shared", 6),
        )
    }

    private fun showWithConversations(): Store {
        withCalls()
        val store = store()
        store.receive(StreamMessage.ConversationChanged(MyceliumJson.decodeFromString(Conversation.serializer(), conversationJson("k1", "Triage support inbox", "claude"))))
        store.receive(StreamMessage.ConversationChanged(MyceliumJson.decodeFromString(Conversation.serializer(), conversationJson("k2", "Team decisions", "codex"))))

        return show(store)
    }

    @Test
    fun theUsageTabCountsCallsPerConversationAndLinksToTheConversation() {
        showWithConversations()
        openCard("Email")
        rule.onNodeWithText("Usage").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Triage support inbox").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("2, 1").assertExists()

        rule.onNodeWithContentDescription("Open Triage support inbox").performClick()
        assertEquals(listOf("k1"), opened.toList())

        rule.onNodeWithContentDescription("Account: Every account").performClick()
        rule.onNodeWithText("b@example.com").performClick()
        rule.onNodeWithText("Triage support inbox").assertDoesNotExist()
        rule.onNodeWithText("Team decisions").assertExists()
    }

    @Test
    fun theActivityLogFiltersByDecisionIntegrationAndConversationAndLinksToTheConversation() {
        showWithConversations()
        rule.onNodeWithText("Activity").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Team decisions").fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithText("search_messages").assertCountEquals(3)

        rule.onNodeWithContentDescription("Decision: Any decision").performClick()
        rule.onAllNodesWithText("Declined").onLast().performClick()
        rule.onAllNodesWithText("send_message").assertCountEquals(1)
        rule.onAllNodesWithText("search_messages").assertCountEquals(0)

        rule.onNodeWithContentDescription("Decision: Declined").performClick()
        rule.onAllNodesWithText("Any decision").onLast().performClick()
        rule.onNodeWithContentDescription("Integration: All integrations").performClick()
        rule.onAllNodesWithText("Zulip").onLast().performClick()
        rule.onAllNodesWithText("get_messages").assertCountEquals(1)
        rule.onAllNodesWithText("search_messages").assertCountEquals(0)

        rule.onNodeWithContentDescription("Integration: Zulip").performClick()
        rule.onAllNodesWithText("All integrations").onLast().performClick()
        rule.onNodeWithContentDescription("Filter by conversation").performTextInput("triage")
        rule.onAllNodesWithText("get_messages").assertCountEquals(0)
        rule.onAllNodesWithText("send_message").assertCountEquals(1)

        rule.onNodeWithContentDescription("send_message on Email a").performClick()
        assertEquals(listOf("k1"), opened.toList())
    }
}
