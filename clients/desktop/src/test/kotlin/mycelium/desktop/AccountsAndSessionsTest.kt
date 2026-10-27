package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.MyceliumApi
import mycelium.state.Choice
import mycelium.state.DefaultAccountId
import mycelium.state.SignInPrefix
import mycelium.state.SessionOptions
import mycelium.state.Store
import mycelium.state.accountChoices
import mycelium.state.accountsElsewhere
import mycelium.ui.AccountsSettings
import mycelium.ui.Composer
import mycelium.ui.MyceliumTheme
import mycelium.ui.NodesScreen
import mycelium.ui.SessionsSettings
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val NodesJson = """[
{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","address":"100.64.4.18","online":true,"draining":false,
 "agents":[{"agent":"claude","version":"2.1.308","available":true}],
 "logins":[{"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"signed_in"},{"agent":"claude","account":"work@acme.example","method":"oauth_token","state":"signed_in"}]},
{"id":"n2","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","address":"10.0.0.12","online":false,"last_seen_at":"2026-10-22T10:00:00Z","draining":false,
 "agents":[{"agent":"claude","version":"2.1.308","available":true}],
 "logins":[{"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"expired"}]}]"""

private const val CodexNodesJson = """[
{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","address":"100.64.4.18","online":true,"draining":false,
 "agents":[{"agent":"codex","version":"0.1.0","available":true}],
 "logins":[{"agent":"codex","account":"sam@example.com","method":"oauth_token","state":"signed_in"}]},
{"id":"n2","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","address":"10.0.0.12","online":true,"draining":false,
 "agents":[{"agent":"codex","version":"0.1.0","available":true}],
 "logins":[{"agent":"codex","method":"oauth_token","state":"signed_out"}]},
{"id":"n3","name":"orion","hostname":"orion","os":"linux","arch":"x86_64","version":"0.1.0","address":"10.0.0.13","online":true,"draining":false,
 "agents":[{"agent":"codex","version":"0.1.0","available":true}],
 "logins":[{"agent":"codex","method":"oauth_token","state":"signed_out"}]}]"""

private const val CodexMissingOnAtlasJson = """[
{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","address":"100.64.4.18","online":true,"draining":false,
 "agents":[{"agent":"codex","version":"0.1.0","available":true}],
 "logins":[{"agent":"codex","account":"sam@example.com","method":"oauth_token","state":"signed_in"}]},
{"id":"n2","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","address":"10.0.0.12","online":true,"draining":false,
 "agents":[{"agent":"codex","available":false}],
 "logins":[]}]"""

private const val SessionsJson = """[
{"id":"s1","kind":"browser","label":"Firefox on Linux","hostname":"","address":"100.64.4.18","last_used_at":"2026-10-23T10:00:00Z","current":true},
{"id":"s2","kind":"client","label":"Desktop app","hostname":"willow","address":"100.64.4.18","last_used_at":"2026-10-23T09:00:00Z","current":false}]"""

private object AccountsPlatform : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

class AccountsAndSessionsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.concurrent.CopyOnWriteArrayList<HttpRequestData>()
    private val bodies = java.util.Collections.synchronizedList(mutableListOf<String>())

    private fun store(nodesJson: String = NodesJson): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            requests += request
            val path = request.url.encodedPath

            when {
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                path.endsWith("/logins") -> {
                    bodies += String(request.body.toByteArray())
                    respond("""{"login_id":"l1"}""", HttpStatusCode.OK, json)
                }
                path.endsWith("/sessions") -> respond(SessionsJson, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(nodesJson, HttpStatusCode.OK, json)
                else -> respond("[]", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }

        return store
    }

    private fun composerWith(accounts: List<Choice>, started: Boolean, onSetAccount: (String) -> Unit = {}) {
        val conversation = Conversation("c1", "u", "n1", "claude", "/work", "t", ConversationStatus.Done, acpSessionId = if (started) "s" else null, account = "work@acme.example", createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")

        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = false, queue = emptyList(), sending = emptyList(), onSend = {}, onStop = {},
                    onSendQueuedNow = {}, onEditQueued = { _, _ -> }, onRemoveQueued = {},
                    options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                    accounts = accounts, accountLocked = started, onSetAccount = onSetAccount,
                )
            }
        }
    }

    @Test
    fun anAccountSignedInOnSeveralNodesIsListedOnceWithTheStateOnEachNode() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        assertEquals(1, rule.onAllNodesWithText("sam@example.com").fetchSemanticsNodes().size)
        rule.onNodeWithText("work@acme.example").assertExists()
        assertEquals(2, rule.onAllNodesWithText("willow").fetchSemanticsNodes().size)
        rule.onNodeWithText("atlas: expired").assertExists()
    }

    @Test
    fun unusedSignedOutNodesStayOutOfTheTableAndSignInIsInTheHarnessMenu() {
        val store = store(CodexNodesJson)
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        assertEquals(1, rule.onAllNodesWithText("sam@example.com").fetchSemanticsNodes().size)
        rule.onNodeWithText("Signed out on atlas, orion").assertDoesNotExist()
        assertEquals(0, rule.onAllNodesWithText("Account name not reported").fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("More for Codex").performClick()
        rule.onNodeWithText("Sign in on orion").performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/nodes/n3/logins") } }
    }

    @Test
    fun signInForCodexIsNotOfferedOnANodeWithoutTheCodexCli() {
        val store = store(CodexMissingOnAtlasJson)
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.onNodeWithContentDescription("More for Codex").performClick()

        rule.onNodeWithText("Sign in on willow").assertExists()
        rule.onNodeWithText("Sign in on atlas").assertDoesNotExist()
    }

    @Test
    fun accountsKnownElsewhereAreOnlyOfferedForSignInOnANodeWithTheCodexCli() {
        val store = store(CodexMissingOnAtlasJson)
        val nodes = store.state.value.nodes
        val atlas = nodes.getValue("n2")
        val willow = nodes.getValue("n1")

        assertEquals(emptyList(), accountsElsewhere(nodes.values, atlas, "codex"))
        assertEquals(emptyList(), accountsElsewhere(nodes.values, willow, "codex"))
        assertEquals(listOf("sam@example.com"), accountsElsewhere(nodes.values, atlas.copy(agents = willow.agents), "codex"))
    }

    @Test
    fun sessionsListWhereEachClientIsAndMarkTheOneInUse() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { SessionsSettings(store, onSignOut = {}) } }

        rule.onNodeWithText("Firefox on Linux").assertExists()
        rule.onNodeWithText("This session").assertExists()
        rule.onNodeWithText("Desktop app on willow").assertExists()
        rule.onNodeWithText("10.0.0.12").assertExists()
    }

    @Test
    fun endingAnotherSessionRemovesItAtOnce() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { SessionsSettings(store, onSignOut = {}) } }

        rule.onNodeWithContentDescription("More for Desktop app on willow").performClick()
        rule.onNodeWithText("End session").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Desktop app on willow").fetchSemanticsNodes().isEmpty() }
        assertEquals("/api/v1/sessions/s2", requests.last { it.method == HttpMethod.Delete }.url.encodedPath)
    }

    @Test
    fun endingTheSessionInUseAsksFirstThenSignsOut() {
        val store = store()
        var signedOut = 0
        rule.setContent { MyceliumTheme(true) { SessionsSettings(store, onSignOut = { signedOut++ }) } }

        rule.onNodeWithContentDescription("More for Firefox on Linux").performClick()
        rule.onNodeWithText("End session").performClick()

        rule.onNodeWithText("Sign out of this session?").assertExists()
        assertTrue(requests.none { it.method == HttpMethod.Delete })

        rule.onNodeWithText("Sign out").performClick()

        rule.waitUntil(5_000) { signedOut == 1 }
        assertEquals("true", requests.last { it.method == HttpMethod.Delete }.url.parameters["confirm_current"])
    }

    @Test
    fun revokingANodeAsksFirst() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { SessionsSettings(store, onSignOut = {}) } }

        rule.onNodeWithContentDescription("More for atlas").performClick()
        rule.onNodeWithText("Revoke").performClick()
        assertTrue(requests.none { it.method == HttpMethod.Delete })

        rule.onNodeWithText("Revoke atlas").performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Delete } }
        assertEquals("/api/v1/nodes/n2", requests.last { it.method == HttpMethod.Delete }.url.encodedPath)
    }

    @Test
    fun theComposerShowsNoAccountPickerWithoutTwoAccounts() {
        composerWith(accounts = emptyList(), started = false)

        rule.onNodeWithContentDescription("Account: work@acme.example").assertDoesNotExist()
        rule.onNodeWithText("Account").assertDoesNotExist()
    }

    @Test
    fun aNewSessionCanPickAnotherAccount() {
        var picked: String? = null
        composerWith(accounts = listOf(Choice("sam@example.com", "sam@example.com"), Choice("work@acme.example", "work@acme.example")), started = false, onSetAccount = { picked = it })

        rule.onNodeWithContentDescription("Account: work@acme.example").performClick()
        rule.onNodeWithText("sam@example.com").performClick()

        assertEquals("sam@example.com", picked)
    }

    @Test
    fun aStartedSessionShowsItsAccountWithoutAPicker() {
        composerWith(accounts = listOf(Choice("sam@example.com", "sam@example.com"), Choice("work@acme.example", "work@acme.example")), started = true)

        rule.onNodeWithContentDescription("Account: work@acme.example, fixed once the conversation has started").assertExists()
    }

    @Test
    fun signingInOnANodeStartsASignInForANewAccountFolder() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.onNodeWithContentDescription("More for Claude Code").performClick()
        rule.onNodeWithText("Sign in on willow").performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/nodes/n1/logins") } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Sign in Claude Code").fetchSemanticsNodes().isNotEmpty() }
        assertTrue(Regex(""""account":"account-\d{4}"""").containsMatchIn(bodies.last()), bodies.last())
        assertTrue(requests.last { it.method == HttpMethod.Post }.url.encodedPath.endsWith("/nodes/n1/logins"))
    }

    @Test
    fun thePickerOffersToSignInOnThisNodeToAnAccountKnownElsewhere() {
        var picked: String? = null
        composerWith(
            accounts = listOf(Choice("sam@example.com", "sam@example.com"), Choice("work@acme.example", "work@acme.example"), Choice(SignInPrefix + "side@example.com", "side@example.com", "Sign in on willow")),
            started = false,
            onSetAccount = { picked = it },
        )

        rule.onNodeWithContentDescription("Account: work@acme.example").performClick()
        rule.onNodeWithText("Sign in on willow").performClick()

        assertEquals("sign in:side@example.com", picked)
    }

    @Test
    fun thePickerIsTitledForTheAgentAndShowsEachAccountsUsageLeftOrThatItIsUnknown() {
        val logins = listOf(
            LoginInfo("claude", "sam@example.com", "token", LoginState.SignedIn, usageLeft = 18),
            LoginInfo("claude", "work@acme.example", "token", LoginState.SignedIn),
        )
        composerWith(accounts = accountChoices(logins, listOf("side@example.com"), "willow"), started = false)

        rule.onNodeWithContentDescription("Account: work@acme.example").performClick()

        rule.onNodeWithText("Claude account").assertExists()
        rule.onNodeWithText("18% left").assertExists()
        rule.onNodeWithText("Usage unknown").assertExists()
        rule.onNodeWithText("Sign in on willow").assertExists()
    }

    @Test
    fun theDefaultAccountIsAnExplicitChoice() {
        var picked: String? = null
        composerWith(accounts = listOf(Choice(DefaultAccountId, "Default account"), Choice("work@acme.example", "work@acme.example")), started = false, onSetAccount = { picked = it })

        rule.onNodeWithContentDescription("Account: work@acme.example").performClick()
        rule.onNodeWithText("Default account").performClick()

        assertEquals(DefaultAccountId, picked)
    }

    @Test
    fun theNodesPageListsEveryAccountOfAnAgentOnANode() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NodesScreen(store, state, AccountsPlatform) } }

        rule.onNodeWithText("work@acme.example").assertExists()
        assertEquals(2, rule.onAllNodesWithText("sam@example.com").fetchSemanticsNodes().size)
    }
}
