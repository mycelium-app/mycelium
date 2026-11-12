package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.setValue
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasText
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.filterToOne
import androidx.compose.ui.test.isSelected
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onChildren
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotDisplayed
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.TextContent
import io.ktor.http.headersOf
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import mycelium.PageReading
import mycelium.Platform
import mycelium.api.ChatToolServer
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.api.OutlineAnswer
import mycelium.api.OutlineCollection
import mycelium.api.OutlineDocumentNode
import mycelium.state.OutlinePage
import mycelium.state.Store
import mycelium.ui.Label
import mycelium.ui.MyTheme
import mycelium.ui.OutlineBrowser
import mycelium.ui.OutlineTreeState
import mycelium.ui.ConversationScreen
import mycelium.ui.LocalOpenList
import mycelium.ui.LocalPlatform
import mycelium.ui.LocalRetainedViewKey
import mycelium.ui.LocalPanelBack
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.OutlinePanel
import mycelium.ui.OutlinePhone
import mycelium.ui.OutlinePhoneStage
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.ui.rememberOutlineBrowser
import org.junit.Rule
import javax.imageio.ImageIO
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val WaitMillis = 60_000L

private const val HandbookId = "col-handbook"

private const val ReleasesId = "col-releases"

private const val HandbookTree = """{"data":[
{"id":"d-eng","title":"Engineering","url":"/doc/engineering-e1","children":[
  {"id":"d-deploy","title":"Deployment guide","url":"/doc/deployment-guide-a1B2c3D","children":[]},
  {"id":"d-oncall","title":"On-call","url":"/doc/on-call-o1","children":[
    {"id":"d-rota","title":"Rota","url":"/doc/rota-r1","children":[]}
  ]}
]},
{"id":"d-norms","title":"Team norms","url":"/doc/team-norms-t1","children":[]}
]}"""

private const val ReleasesTree = """{"data":[{"id":"d-notes","title":"harbour release notes, v0.4.0","url":"/doc/harbour-notes-h1","children":[]}]}"""

private val DocsAccount = outlineAccount("s1", "Docs", "https://docs.example.org/mcp")

private val WikiAccount = outlineAccount("s2", "Team wiki", "https://wiki.example.org/mcp")

private fun outlineAccount(id: String, name: String, url: String) = ChatToolServer(id, name, url = url, settings = JsonObject(mapOf("app" to JsonPrimitive("outline"))), createdAt = "2026-11-01T10:00:00Z")

private class OutlinePlatform(private val inner: AppsPlatform = AppsPlatform(), override val embedsOtherSites: Boolean = true) : Platform by inner {
    val shown = java.util.concurrent.CopyOnWriteArrayList<String>()
    val opened: List<String> get() = inner.opened
    val retainKeys = java.util.concurrent.CopyOnWriteArrayList<String?>()
    var reading: PageReading? = null

    @Composable
    override fun WebContent(url: String, token: String?, modifier: Modifier, reading: PageReading?) {
        this.reading = reading
        retainKeys += LocalRetainedViewKey.current
        LaunchedEffect(url) { shown += url }
        Box(modifier.background(Color.White).padding(24.dp)) {
            Label("Outline page (test stub)", color = Color.Black)
        }
    }
}

class OutlinePanelTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val platform = OutlinePlatform()
    private val firstFrameDrawn = CompletableDeferred<Unit>()

    private fun answerRequests() {
        rule.waitForIdle()
        firstFrameDrawn.complete(Unit)
    }

    private fun outlineEngine(documents: (String) -> Pair<HttpStatusCode, String> = { body -> HttpStatusCode.OK to if (ReleasesId in body) ReleasesTree else HandbookTree }) = MockEngine { request ->
        firstFrameDrawn.await()

        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val path = request.url.encodedPath

        when {
            path.endsWith("/s1/proxy/collections.list") -> respond("""{"data":[{"id":"$HandbookId","name":"Handbook"},{"id":"$ReleasesId","name":"Releases"}]}""", HttpStatusCode.OK, json)
            path.endsWith("/s2/proxy/collections.list") -> respond("""{"data":[{"id":"col-product","name":"Product"}]}""", HttpStatusCode.OK, json)
            path.endsWith("/proxy/collections.documents") -> documents((request.body as TextContent).text).let { (status, body) -> respond(body, status, json) }
            else -> respond("{}", HttpStatusCode.OK, json)
        }
    }

    private fun showPanel(accounts: List<ChatToolServer> = listOf(DocsAccount), start: OutlinePage? = null, engine: MockEngine = outlineEngine(), height: Int = 860) {
        val api = MyceliumApi("https://mycelium.test", "token", engine)

        rule.setContent {
            MyceliumTheme(true) {
                var page by remember { mutableStateOf(start) }
                val browser = rememberOutlineBrowser("c1", api, accounts, page, true)

                Box(Modifier.width(480.dp).height(height.dp)) {
                    OutlinePanel(browser, platform, accounts, page, { page = it }, onClose = {})
                }
            }
        }

        answerRequests()
    }

    private fun waitForText(text: String) {
        rule.waitUntil(WaitMillis) { rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
    }

    private fun waitForAbsence(text: String) {
        rule.waitUntil(WaitMillis) { rule.onAllNodesWithText(text).fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun choosingARowLoadsThatDocumentBelowTheTree() {
        showPanel()

        waitForText("Team norms")
        rule.onNodeWithContentDescription("Outline collection Handbook. Change").assertExists()
        rule.onNodeWithText("Team norms").performClick()

        rule.waitUntil(WaitMillis) { platform.shown.isNotEmpty() }
        assertEquals(listOf("https://docs.example.org/doc/team-norms-t1"), platform.shown.toList())
        rule.waitForIdle()
        rule.onNodeWithContentDescription("Outline collection Handbook. Change").assertExists()
        rule.onAllNodesWithText("Team norms").assertCountEquals(2)
        rule.captureScreen("outline-tree-and-document")
        rule.onNodeWithContentDescription("Open in browser").assertExists()
    }

    @Test
    fun theTreeIsOperatedFromTheKeyboardAlone() {
        showPanel()

        waitForText("Engineering")
        val tree = rule.onNodeWithContentDescription("Documents")

        tree.requestFocus()
        tree.performKeyInput { pressKey(Key.DirectionRight) }
        waitForText("Deployment guide")

        tree.performKeyInput {
            pressKey(Key.DirectionRight)
            pressKey(Key.Enter)
        }

        rule.waitUntil(WaitMillis) { platform.shown.isNotEmpty() }
        assertEquals("https://docs.example.org/doc/deployment-guide-a1B2c3D", platform.shown.single())
        rule.onAllNodesWithText("Deployment guide").filterToOne(isSelected()).assertExists()

        tree.performKeyInput {
            pressKey(Key.DirectionLeft)
            pressKey(Key.DirectionLeft)
        }

        waitForAbsence("On-call")
        rule.onNodeWithText("Engineering").assertIsNotSelected()
    }

    @Test
    fun anAgentOpeningADocumentMovesToItsCollectionAndExpandsItsParents() {
        showPanel(start = OutlinePage("Rota", "https://docs.example.org/doc/rota-r1", "s1", "d-rota", HandbookId))

        rule.waitUntil(WaitMillis) { rule.onAllNodesWithText("Rota").fetchSemanticsNodes().size == 2 }
        rule.waitForIdle()
        rule.onAllNodesWithText("Rota").filterToOne(isSelected()).assertExists()
        rule.onNodeWithContentDescription("Outline collection Handbook. Change").assertExists()
    }

    @Test
    fun theSinglePickerChoosesAnAccountAndCollection() {
        showPanel(listOf(DocsAccount, WikiAccount))

        waitForText("Engineering")
        rule.onNodeWithContentDescription("Outline account Docs, collection Handbook. Change").assertExists()
        rule.onNodeWithContentDescription("Outline account Docs, collection Handbook. Change").performClick()
        rule.captureScreen("outline-picker")
        rule.onNodeWithText("Releases").performClick()

        waitForText("harbour release notes, v0.4.0")
        rule.onNodeWithContentDescription("Outline account Docs, collection Releases. Change").assertExists()
    }

    @Test
    fun theTreeCollapsesAndExpandsFromTheBarAndKeepsTheDocument() {
        showPanel(start = OutlinePage("Team norms", "https://docs.example.org/doc/team-norms-t1", "s1", "d-norms", HandbookId))

        waitForText("Engineering")
        rule.onNodeWithContentDescription("Collapse the document tree").performClick()

        waitForAbsence("Engineering")
        rule.onNodeWithText("Team norms").assertExists()
        assertEquals(listOf("https://docs.example.org/doc/team-norms-t1"), platform.shown.toList())

        rule.captureScreen("outline-collapsed")
        rule.onNodeWithContentDescription("Expand the document tree").performClick()
        waitForText("Engineering")
    }

    @Test
    fun enterOnTheDividerCollapsesTheTree() {
        showPanel()

        waitForText("Engineering")
        val divider = rule.onNodeWithContentDescription("Resize the document tree").onChildren().onFirst()

        divider.requestFocus()
        divider.performKeyInput { pressKey(Key.Enter) }

        waitForAbsence("Engineering")
        rule.onNodeWithContentDescription("Expand the document tree").assertExists()
    }

    @Test
    fun aShortWindowStartsWithTheTreeCollapsed() {
        showPanel(height = 400)

        rule.waitUntil(WaitMillis) { runCatching { rule.onNodeWithContentDescription("Expand the document tree").fetchSemanticsNode() }.isSuccess }
    }

    @Test
    fun anEmptyCollectionSaysDocumentsAppearOnceAddedInOutline() {
        showPanel(engine = outlineEngine { HttpStatusCode.OK to """{"data":[]}""" })

        waitForText("Handbook has no documents yet")
        rule.onNodeWithText("Documents added in Outline appear here.").assertExists()
        rule.captureScreen("outline-empty")
    }

    @Test
    fun anUnreachableOutlineOffersTryAgainWhichReloadsTheTree() {
        var failing = true

        showPanel(engine = outlineEngine { if (failing) HttpStatusCode.BadGateway to """{"error":"integration_unreachable","message":"x"}""" else HttpStatusCode.OK to HandbookTree })

        waitForText("Mycelium could not reach Docs")
        rule.onNodeWithText("Check that docs.example.org is online, then try again.").assertExists()
        rule.captureScreen("outline-unreachable")

        failing = false
        rule.onNodeWithText("Try again").performClick()

        waitForText("Engineering")
    }

    @Test
    fun aRefusedKeyPointsAtTheIntegrationPage() {
        showPanel(engine = outlineEngine { HttpStatusCode.Unauthorized to """{"error":"authentication_required","message":"x"}""" })

        waitForText("Outline refused the API key for Docs")
        rule.onNodeWithText("Open Docs integration").assertExists()
        rule.captureScreen("outline-refused")
    }

    @Test
    fun overTheChatInANarrowWindowThePanelGoesBackInsteadOfClosing() {
        val api = MyceliumApi("https://mycelium.test", "token", outlineEngine())
        val accounts = listOf(DocsAccount)
        var wentBack = false

        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalPanelBack provides { wentBack = true }, LocalOpenList provides {}) {
                    val page = OutlinePage("Team norms", "https://docs.example.org/doc/team-norms-t1", "s1", "d-norms", HandbookId)
                    val browser = rememberOutlineBrowser("c1", api, accounts, page, true)

                    Box(Modifier.width(760.dp).height(860.dp)) { OutlinePanel(browser, platform, accounts, page, {}, onClose = {}) }
                }
            }
        }

        answerRequests()
        waitForText("Engineering")
        rule.captureScreen("outline-narrow")

        assertTrue(runCatching { rule.onNodeWithContentDescription("Close Outline").fetchSemanticsNode() }.isFailure)
        rule.onNodeWithContentDescription("Back to the conversation").performClick()
        assertTrue(wentBack)
    }

    @Test
    fun aGroupWithNoOutlineAccountOffersToAddOne() {
        showPanel(emptyList())

        waitForText("No Outline account in this group")
        rule.onNodeWithText("Add Outline integration").assertExists()
        rule.captureScreen("outline-no-account")
    }

    @Test
    fun theSignInNoteAppearsWhileThePanelShowsOutlinesSignInPage() {
        showPanel(start = OutlinePage("Team norms", "https://docs.example.org/doc/team-norms-t1", "s1", "d-norms", HandbookId))

        rule.waitUntil(WaitMillis) { platform.reading != null }
        platform.reading!!.onRead("/")
        waitForText("Sign in once on this device to open documents here.")
        rule.captureScreen("outline-sign-in")

        platform.reading!!.onRead("/doc/team-norms-t1")
        waitForAbsence("Sign in once on this device to open documents here.")
    }

    @Test
    fun onThePhoneARowOpensTheDocumentScreenAndBackReturnsToTheTree() {
        val api = MyceliumApi("https://mycelium.test", "token", outlineEngine())
        val accounts = listOf(DocsAccount, WikiAccount)

        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalTouch provides true) {
                    var page by remember { mutableStateOf<OutlinePage?>(null) }
                    var stage by remember { mutableStateOf(OutlinePhoneStage.Tree) }
                    val browser = rememberOutlineBrowser("c1", api, accounts, page, true)

                    Column { OutlinePhone(browser, platform, accounts, page, stage, { stage = it }, { page = it }, onClose = {}) }
                }
            }
        }

        answerRequests()

        waitForText("Team norms")
        rule.onNodeWithContentDescription("Back to the conversation").assertExists()
        rule.onNodeWithContentDescription("Outline account Docs, collection Handbook. Change").performClick()
        rule.onNode(hasText("Handbook") and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.RadioButton)).assertIsSelected()
        rule.onNode(hasText("Product") and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.RadioButton)).assertIsNotSelected()
        assertTrue(rule.onAllNodesWithText("Find a collection").fetchSemanticsNodes().isEmpty())
        rule.onNode(hasText("Handbook") and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.RadioButton)).performClick()
        rule.onNodeWithText("Team norms").performClick()

        rule.waitUntil(WaitMillis) { platform.shown.isNotEmpty() }
        rule.waitUntil(WaitMillis) { platform.reading != null }
        platform.reading!!.onRead("/")
        waitForText("Sign in once on this device to open documents here.")
        rule.onNodeWithText("Docs").assertExists()
        platform.reading!!.onRead("/doc/team-norms-t1")
        waitForAbsence("Sign in once on this device to open documents here.")
        rule.onNodeWithContentDescription("Back to the document tree").performClick()
        waitForText("Engineering")
    }

    private fun showConversation(platform: Platform, servers: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val conversation = """{"id":"c1","owner":"u1","node_id":"n1","agent":"claude","cwd":"/work","title":"c1","status":"done","acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""
        val outline = outlineEngine()
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                "/proxy/" in path -> outline.config.requestHandlers.first()(this, request)
                path.endsWith("/chat-tool-servers") -> respond(servers, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[$conversation]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(conversation, HttpStatusCode.OK, json)
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/events") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking {
            store.refresh()
            store.refreshChatToolServers()
        }

        rule.setContent {
            MyceliumTheme(true) {
                Shell(store, platform, Me("u1", "person@example.test", "Person"), onSignOut = {}, initialRoute = Route.Chat("c1"))
            }
        }

        answerRequests()
    }

    private val docsServerJson = """[{"id":"s1","name":"Docs","kind":"mcp","url":"https://docs.example.org/mcp","settings":{"app":"outline"},"created_at":"2026-11-01T10:00:00Z"}]"""

    @Test
    fun theRailOffersOutlineOnlyWhenTheGroupHasAnAccount() {
        showConversation(platform, docsServerJson)

        rule.waitUntil(WaitMillis) { runCatching { rule.onNodeWithContentDescription("Open Outline").fetchSemanticsNode() }.isSuccess }
        rule.onNodeWithContentDescription("Open Outline").performClick()

        waitForText("Engineering")
        rule.onAllNodesWithContentDescription("Close Outline").assertCountEquals(2)
    }

    @Test
    fun theRailHasNoOutlineButtonWithoutAnAccount() {
        showConversation(platform, "[]")

        rule.waitUntil(WaitMillis) { runCatching { rule.onNodeWithContentDescription("Open git").fetchSemanticsNode() }.isSuccess }
        assertTrue(runCatching { rule.onNodeWithContentDescription("Open Outline").fetchSemanticsNode() }.isFailure)
    }

    @Test
    fun onTheWebClientARowOpensANewTabAndTheTreeFillsThePanel() {
        val web = OutlinePlatform(embedsOtherSites = false)

        showConversation(web, docsServerJson)

        rule.waitUntil(WaitMillis) { runCatching { rule.onNodeWithContentDescription("Open Outline").fetchSemanticsNode() }.isSuccess }
        rule.onNodeWithContentDescription("Open Outline").performClick()

        waitForText("Team norms")
        rule.onNodeWithText("Outline does not allow its pages inside another site, so documents open in a new tab.").assertExists()
        assertTrue(runCatching { rule.onNodeWithContentDescription("Collapse the document tree").fetchSemanticsNode() }.isFailure)
        rule.captureScreen("outline-web")

        rule.onNodeWithText("Team norms").performClick()

        rule.waitUntil(WaitMillis) { web.opened.isNotEmpty() }
        assertEquals(listOf("https://docs.example.org/doc/team-norms-t1"), web.opened)
        assertTrue(web.shown.isEmpty())
    }

    private fun longHandbook(): String {
        val fillers = (1..40).joinToString(",") { """{"id":"d-f$it","title":"Filler $it","url":"/doc/filler-$it","children":[]}""" }

        return HandbookTree.removeSuffix("]}") + "," + fillers + "]}"
    }

    private fun showSwitchableConversations(): (String) -> Unit {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val conversation = { id: String -> """{"id":"$id","owner":"u1","node_id":"n1","agent":"claude","cwd":"/work","title":"$id","status":"done","acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}""" }
        val outline = outlineEngine { HttpStatusCode.OK to longHandbook() }
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                "/proxy/" in path -> outline.config.requestHandlers.first()(this, request)
                path.endsWith("/chat-tool-servers") -> respond(docsServerJson, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[${conversation("c1")},${conversation("c2")}]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(conversation("c1"), HttpStatusCode.OK, json)
                path.endsWith("/conversations/c2") -> respond(conversation("c2"), HttpStatusCode.OK, json)
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/events") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        var switchTo: (String) -> Unit = {}

        runBlocking {
            store.refresh()
            store.refreshChatToolServers()
        }

        rule.setContent {
            MyceliumTheme(true) {
                var id by remember { mutableStateOf("c1") }
                val state by store.state.collectAsState()

                switchTo = { id = it }

                CompositionLocalProvider(LocalPlatform provides platform) {
                    Box(Modifier.width(1100.dp).height(860.dp)) {
                        ConversationScreen(store, state, id, onArchived = {}, onOpenConversation = { id = it })
                    }
                }
            }
        }

        answerRequests()

        return { switchTo(it) }
    }

    @Test
    fun switchingToAnotherConversationAndBackKeepsTheOutlinePanelAsItWas() {
        val switchTo = showSwitchableConversations()

        rule.waitUntil(WaitMillis) { runCatching { rule.onNodeWithContentDescription("Open Outline").fetchSemanticsNode() }.isSuccess }
        rule.onNodeWithContentDescription("Open Outline").performClick()
        waitForText("Engineering")

        val tree = rule.onNodeWithContentDescription("Documents")

        tree.requestFocus()
        tree.performKeyInput {
            pressKey(Key.DirectionRight)
            pressKey(Key.DirectionRight)
            pressKey(Key.DirectionDown)
            pressKey(Key.DirectionRight)
            pressKey(Key.DirectionDown)
            pressKey(Key.Enter)
        }

        rule.waitUntil(WaitMillis) { platform.shown.isNotEmpty() }
        rule.onAllNodesWithText("Rota").filterToOne(isSelected()).assertExists()
        rule.onNodeWithText("Filler 40").performScrollTo()
        rule.onNodeWithText("Engineering").assertIsNotDisplayed()

        switchTo("c2")
        rule.waitForIdle()
        assertTrue(rule.onAllNodesWithContentDescription("Close Outline").fetchSemanticsNodes().isEmpty())

        switchTo("c1")
        waitForText("Rota")

        rule.onAllNodesWithContentDescription("Close Outline").assertCountEquals(2)
        rule.onNodeWithContentDescription("Outline collection Handbook. Change").assertExists()
        rule.onAllNodesWithText("Rota").filterToOne(isSelected()).assertExists()
        rule.onNodeWithText("On-call").assertExists()
        rule.onNodeWithText("Filler 40").assertIsDisplayed()
        rule.onNodeWithText("Engineering").assertIsNotDisplayed()
        assertEquals(setOf("outline:c1"), platform.retainKeys.toSet())
        assertEquals(listOf("https://docs.example.org/doc/rota-r1"), platform.shown.distinct())
    }
}

@OptIn(InternalComposeUiApi::class)
class OutlineRenderTest {
    private fun phone(name: String, stage: OutlinePhoneStage = OutlinePhoneStage.Tree, sheet: Boolean = false, signedOut: Boolean = false) {
        val platform = OutlinePlatform()
        val accounts = listOf(DocsAccount, WikiAccount)
        val page = OutlinePage("Deployment guide", "https://docs.example.org/doc/deployment-guide-a1B2c3D", "s1", "d-deploy", HandbookId)
        val browser = OutlineBrowser().apply {
            collections = mapOf(
                "s1" to listOf(OutlineCollection(HandbookId, "Handbook"), OutlineCollection(ReleasesId, "Releases")),
                "s2" to listOf(OutlineCollection("col-product", "Product")),
            )
            choose("s1", HandbookId)
            tree = OutlineTreeState.Loaded(Json.decodeFromString<OutlineAnswer<OutlineDocumentNode>>(HandbookTree).data)
            expanded = setOf("d-eng")
        }
        val out = renderAt(name, dark = true, phone = true, interact = { scene ->
            if (sheet) {
                for (key in listOf(Key.Tab, Key.Tab, Key.Enter)) {
                    scene.sendKeyEvent(KeyEvent(key, KeyEventType.KeyDown))
                    scene.sendKeyEvent(KeyEvent(key, KeyEventType.KeyUp))
                }
            }

            if (signedOut) {
                requireNotNull(platform.reading).onRead("/")
            }
        }) {
            Column(Modifier.fillMaxSize().background(MyTheme.colors.bgPanel)) {
                OutlinePhone(browser, platform, accounts, page, stage, {}, {}, onClose = {})
            }
        }
        val image = ImageIO.read(out)

        assertEquals(390, image.width)
        assertEquals(844, image.height)

        if (signedOut) {
            assertTrue(browser.signedOut)
        }
    }

    @Test
    fun thePhoneTreeRendersAtTheDesignedSize() {
        phone("outline-tree")
    }

    @Test
    fun thePhonePickerSheetRendersAtTheDesignedSize() {
        phone("outline-picker-sheet", sheet = true)
    }

    @Test
    fun thePhoneDocumentRendersAtTheDesignedSize() {
        phone("outline-document", stage = OutlinePhoneStage.Document)
    }

    @Test
    fun thePhoneSignInNoticeRendersAtTheDesignedSize() {
        phone("outline-sign-in", stage = OutlinePhoneStage.Document, signedOut = true)
    }
}
