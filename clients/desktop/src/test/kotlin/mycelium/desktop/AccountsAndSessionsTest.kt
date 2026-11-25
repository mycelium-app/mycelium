package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.hasText
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
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
import mycelium.api.MyceliumApi
import mycelium.state.ModelOption
import mycelium.state.PickerAccount
import mycelium.state.DefaultAccountId
import mycelium.state.SignInPrefix
import mycelium.state.SessionOptions
import mycelium.state.Store
import mycelium.state.accountsElsewhere
import mycelium.ui.AccountsSettings
import mycelium.ui.Composer
import mycelium.ui.ModelPickerControls
import mycelium.ui.MyceliumTheme
import mycelium.ui.SessionsSettings
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val NodesJson = """[
{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","address":"100.64.4.18","online":true,"draining":false,
 "agents":[{"agent":"claude","version":"2.1.308","available":true}],
 "logins":[{"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"signed_in"},{"agent":"claude","account":"work@example.net","method":"oauth_token","state":"signed_in"}]},
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

private const val VibeSignedOutJson = """[
{"id":"n1","name":"orion","hostname":"orion","os":"linux","arch":"x86_64","version":"0.1.0","address":"10.0.0.13","online":true,"draining":false,
 "agents":[{"agent":"vibe","version":"2.0.0","available":true}],
 "logins":[{"agent":"vibe","account":"default","method":"browser","state":"signed_out"}]}]"""

private const val SignInsJson = """[
{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","address":"192.0.2.10","online":true,"draining":false,
 "agents":[{"agent":"claude","version":"2.1.308","available":true},{"agent":"codex","version":"0.1.0","available":true}],
 "logins":[{"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"signed_in"},{"agent":"claude","account":"work@example.net","method":"oauth_token","state":"signed_in"},{"agent":"codex","account":"sam@example.com","method":"device_auth","state":"signed_out"}]},
{"id":"n2","name":"orion","hostname":"orion","os":"linux","arch":"x86_64","version":"0.1.0","address":"192.0.2.11","online":true,"draining":false,
 "agents":[{"agent":"claude","version":"2.1.308","available":true},{"agent":"codex","available":false}],
 "logins":[{"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"expired"}]}]"""

private const val VibeSignedInJson = """[
{"id":"n1","name":"orion","hostname":"orion","os":"linux","arch":"x86_64","version":"0.1.0","address":"192.0.2.11","online":true,"draining":false,
 "agents":[{"agent":"vibe","version":"2.0.0","available":true}],
 "logins":[{"agent":"vibe","account":"work","method":"browser","state":"signed_in","managed":true}]}]"""

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
    private val bodies = java.util.concurrent.CopyOnWriteArrayList<String>()

    private fun store(nodesJson: String = NodesJson): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = SynchronousMockEngine { request ->
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

    private fun logins() = requests.filter { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/logins") }.map { it.url.encodedPath }

    private fun willow() = store().state.value.nodes.values.first { it.name == "willow" }

    private fun account(id: String, name: String?, usage: String = "Usage unknown") = PickerAccount(id, "claude", name, willow(), true, usage)

    private fun composerWith(accounts: List<PickerAccount>, started: Boolean, onSetAccount: (String) -> Unit = {}) {
        val conversation = Conversation("c1", "u", "n1", "claude", "/work", "t", ConversationStatus.Done, acpSessionId = if (started) "s" else null, account = "work@example.net", createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")

        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = false, queue = emptyList(), onSend = {}, onStop = {},
                    onSendQueuedNow = {}, onEditQueued = {}, onRemoveQueued = {},
                    options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                    accountPicker = ModelPickerControls(
                        label = "Claude Opus 5.5",
                        options = listOf(ModelOption("claude", null, "opus", "Claude Opus 5.5")),
                        favourites = emptyList(), providers = emptyList(), onPickModel = {}, onFavourite = { _, _ -> },
                        accounts = accounts, account = "work@example.net",
                        onPickAccount = { picked, done -> onSetAccount(picked.orEmpty()); done(null) },
                    ),
                )
            }
        }
    }

    @Test
    fun anAccountSignedInOnSeveralNodesIsOneRowWithACellPerNode() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        assertEquals(1, rule.onAllNodesWithText("sam@example.com").fetchSemanticsNodes().size)
        rule.onNodeWithText("work@example.net").assertExists()
        rule.onNodeWithContentDescription("sam@example.com: Expired on atlas").assertExists()
        rule.onNodeWithContentDescription("Sign in sam@example.com on atlas").assertDoesNotExist()
        rule.onNodeWithText("Not signed in").assertExists()
    }

    @Test
    fun eachCellSaysWhetherTheAccountIsSignedInOutOrNotInstalled() {
        val store = store(SignInsJson)
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        assertEquals(2, rule.onAllNodesWithText("Signed in").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("sam@example.com: Expired on orion").assertExists()
        rule.onNodeWithContentDescription("sam@example.com: Signed out on willow").assertExists()
        rule.onNodeWithContentDescription("Add work@example.net on orion").assertExists()
        rule.onNodeWithText("Not installed").assertExists()
    }

    @Test
    fun aSignedOutCellShowsItsStateAndSignInWholeInANarrowWindow() {
        val store = store(SignInsJson)
        rule.setContent { MyceliumTheme(true) { Box(Modifier.width(360.dp)) { AccountsSettings(store) } } }

        val words = rule.onAllNodes(hasText("Sign in") or hasText("Signed out") or hasText("Expired"), useUnmergedTree = true).fetchSemanticsNodes()
        val clipped = words.filter { node ->
            val layouts = mutableListOf<TextLayoutResult>()
            node.config[SemanticsActions.GetTextLayoutResult].action?.invoke(layouts)
            layouts.any { it.multiParagraph.intrinsics.maxIntrinsicWidth > it.size.width + 0.5f || it.didOverflowHeight }
        }.map { it.config[SemanticsProperties.Text].joinToString() }

        assertTrue(words.size >= 4, "${words.size}")
        assertEquals(emptyList(), clipped)
    }

    @Test
    fun theBannerCountsTheSignInsThatNeedYouAndSignInToAllStepsThroughEach() {
        val store = store(SignInsJson)
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.onNodeWithText("2 sign-ins need you").assertExists()
        rule.onNodeWithText("Sign in to all 2").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Sign in to Claude Code as sam@example.com on orion").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(listOf("/api/v1/nodes/n2/logins"), logins())
        assertTrue(bodies.last().contains("\"agent\":\"claude\"") && bodies.last().contains("\"account\":\"sam@example.com\""), bodies.last())

        rule.onNodeWithText("Cancel").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Sign in to Codex as sam@example.com", substring = true).fetchSemanticsNodes().isNotEmpty() }
        assertEquals(listOf("/api/v1/nodes/n2/logins", "/api/v1/nodes/n1/logins"), logins())
        assertTrue(bodies.last().contains("\"agent\":\"codex\"") && bodies.last().contains("\"account\":\"sam@example.com\""), bodies.last())

        rule.onNodeWithText("Cancel").performClick()
        rule.waitForIdle()

        assertEquals(2, logins().size)
        assertEquals(0, rule.onAllNodesWithText("Sign in to Codex", substring = true).fetchSemanticsNodes().size)
    }

    @Test
    fun addingAnAccountOnAnotherNodeSignsInThatAccountThere() {
        val store = store(SignInsJson)
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.onNodeWithContentDescription("Add work@example.net on orion").performClick()

        rule.waitUntil(5_000) { logins().isNotEmpty() }
        assertEquals(listOf("/api/v1/nodes/n2/logins"), logins())
        assertTrue(bodies.last().contains("\"account\":\"work@example.net\""), bodies.last())
    }

    @Test
    fun signingInASignedOutMistralVibeAccountSignsInThatAccountOnItsNode() {
        val store = store(VibeSignedOutJson)
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.onNodeWithText("1 sign-in needs you").assertExists()
        rule.onNodeWithContentDescription("Sign in default on orion").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Sign in to Mistral Vibe as default on orion").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(listOf("/api/v1/nodes/n1/logins"), logins())
        assertTrue(bodies.last().contains("\"account\":\"default\""), bodies.last())
        assertTrue(bodies.last().contains("\"agent\":\"vibe\""), bodies.last())
    }

    @Test
    fun aMistralVibeAccountCanBeSignedOutAndRemovedFromItsPage() {
        val store = store(VibeSignedInJson)
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.onNodeWithText("work").performClick()
        rule.onNodeWithContentDescription("More for orion").performClick()
        rule.onNodeWithText("Sign out on orion").performClick()
        rule.onNodeWithText("Sign work out on orion?").assertExists()
        rule.onNodeWithText("Sign out").performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/sign-out") } }
        assertEquals("/api/v1/nodes/n1/logins/vibe/work/sign-out", requests.last { it.method == HttpMethod.Post }.url.encodedPath)

        rule.onNodeWithText("Remove this account").performClick()
        rule.onNodeWithText("Remove work").performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Delete } }
        assertEquals("/api/v1/nodes/n1/logins/vibe/work", requests.last { it.method == HttpMethod.Delete }.url.encodedPath)
    }

    @Test
    fun unusedSignedOutNodesStayOutOfTheTable() {
        val store = store(CodexNodesJson)
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        assertEquals(1, rule.onAllNodesWithText("sam@example.com").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Account name not reported").fetchSemanticsNodes().size)
        rule.onNodeWithText("sign-ins need you", substring = true).assertDoesNotExist()
    }

    @Test
    fun aNewAccountIsOnlyOfferedOnANodeWithTheAgentInstalled() {
        val store = store(CodexMissingOnAtlasJson)
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.onNodeWithContentDescription("More for Codex").performClick()

        rule.onNodeWithText("Sign in a new account on willow").assertExists()
        rule.onNodeWithText("Sign in a new account on atlas").assertDoesNotExist()
    }

    @Test
    fun selectingAnAccountOpensItsPageAndBackReturnsToTheGrid() {
        val store = store(SignInsJson)
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.onNodeWithText("work@example.net").performClick()

        rule.onNodeWithText("Signed in on").assertExists()
        rule.onNodeWithText("Sign in on another node").assertExists()
        rule.onNodeWithText("Change price").assertExists()
        rule.onNodeWithText("Usage history").assertExists()

        rule.onNodeWithText("Sign in on another node").performClick()
        rule.onNodeWithText("orion").performClick()

        rule.waitUntil(5_000) { logins().isNotEmpty() }
        assertTrue(bodies.last().contains("\"account\":\"work@example.net\""), bodies.last())

        rule.onNodeWithText("Cancel").performClick()
        rule.onNodeWithContentDescription("Back to Accounts").performClick()
        rule.onNodeWithText("2 sign-ins need you").assertExists()
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
    fun endingAnotherSessionRemovesItAtOnce() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { SessionsSettings(store, onSignOut = {}) } }

        rule.onNodeWithContentDescription("More for Desktop app on willow").performClick()
        rule.onNodeWithText("End session").performClick()

        rule.waitForIdle()

        assertEquals(0, rule.onAllNodesWithText("Desktop app on willow").fetchSemanticsNodes().size)
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
    fun aNewSessionCanPickAnotherAccount() {
        var picked: String? = null
        composerWith(accounts = listOf(account("sam@example.com", "sam@example.com"), account("work@example.net", "work@example.net")), started = false, onSetAccount = { picked = it })

        rule.onNodeWithContentDescription("Model: Claude Opus 5.5").performClick()
        rule.onNodeWithText("Account").performClick()
        rule.onNodeWithText("Claude Code · sam@example.com on willow").performClick()

        assertEquals("sam@example.com", picked)
    }

    @Test
    fun aStartedSessionCanStillSwitchAccount() {
        var picked: String? = null
        composerWith(accounts = listOf(account("sam@example.com", "sam@example.com"), account("work@example.net", "work@example.net")), started = true, onSetAccount = { picked = it })

        rule.onNodeWithContentDescription("Model: Claude Opus 5.5").performClick()
        rule.onNodeWithText("Account").performClick()
        rule.onNodeWithText("Claude Code · sam@example.com on willow").performClick()

        assertEquals("sam@example.com", picked)
    }

    @Test
    fun signingInANewAccountStartsASignInForANewAccountFolder() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.onNodeWithContentDescription("More for Claude Code").performClick()
        rule.onNodeWithText("Sign in a new account on willow").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Sign in to Claude Code as account-", substring = true).fetchSemanticsNodes().isNotEmpty() }
        assertEquals(listOf("/api/v1/nodes/n1/logins"), logins())
        assertTrue(Regex(""""account":"account-\d{4}"""").containsMatchIn(bodies.last()), bodies.last())
    }

    @Test
    fun thePickerOffersToSignInOnThisNodeToAnAccountKnownElsewhere() {
        var picked: String? = null
        composerWith(
            accounts = listOf(
                account("sam@example.com", "sam@example.com"),
                account("work@example.net", "work@example.net"),
                PickerAccount(SignInPrefix + "side@example.com", "claude", "side@example.com", willow(), true, "Sign in on willow", signIn = true),
            ),
            started = false,
            onSetAccount = { picked = it },
        )

        rule.onNodeWithContentDescription("Model: Claude Opus 5.5").performClick()
        rule.onNodeWithText("Account").performClick()
        rule.onNodeWithText("Sign in on willow · side@example.com").performClick()

        assertEquals("sign in:side@example.com", picked)
    }

    @Test
    fun thePickerShowsEachAccountsUsageLeftOrThatItIsUnknown() {
        composerWith(accounts = listOf(account("sam@example.com", "sam@example.com", "18% left"), account("work@example.net", "work@example.net")), started = false)

        rule.onNodeWithContentDescription("Model: Claude Opus 5.5").performClick()
        rule.onNodeWithText("Account").performClick()

        rule.onNodeWithContentDescription("Claude Code · sam@example.com on willow, 18% left").assertExists()
        rule.onNodeWithContentDescription("Claude Code · work@example.net on willow, Usage unknown").assertExists()
    }

    @Test
    fun theDefaultAccountIsAnExplicitChoice() {
        var picked: String? = null
        composerWith(accounts = listOf(account(DefaultAccountId, null), account("work@example.net", "work@example.net")), started = false, onSetAccount = { picked = it })

        rule.onNodeWithContentDescription("Model: Claude Opus 5.5").performClick()
        rule.onNodeWithText("Account").performClick()
        rule.onNodeWithText("Claude Code · Default account on willow").performClick()

        assertEquals(DefaultAccountId, picked)
    }
}
