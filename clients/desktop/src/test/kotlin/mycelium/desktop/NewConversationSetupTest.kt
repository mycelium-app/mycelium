package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewConversationScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Atlas = "aaaaaaaa-0000-0000-0000-000000000001"
private const val Willow = "aaaaaaaa-0000-0000-0000-000000000002"

private fun node(id: String, name: String) = """{"id":"$id","name":"$name","hostname":"$name","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[],"harness_models":[]}"""

private const val HarbourOnAtlas = """{"id":"p1","node_id":"$Atlas","name":"harbour","path":"/p/harbour","forge_id":"f1","repository":"example-org/harbour","created_at":"2026-10-22T10:00:00Z"}"""

private const val HarbourOnWillow = """{"id":"p2","node_id":"$Willow","name":"harbour","path":"/srv/harbour","forge_id":"f1","repository":"example-org/harbour","created_at":"2026-10-22T10:00:00Z"}"""

private const val CreatedJson = """{"id":"c1","owner":"u","node_id":"$Willow","agent":"claude","cwd":"/srv/harbour","title":"New","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val RecentJson = """[{"id":"c0","owner":"u","node_id":"$Atlas","agent":"claude","cwd":"/p/harbour","title":"Restore flow","status":"done","project_id":"p1",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T11:00:00Z","archived_at":null}]"""

class NewConversationSetupTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val bodies = CopyOnWriteArrayList<String>()
    private val checkouts = CopyOnWriteArrayList<String>()
    private val previewProjects = CopyOnWriteArrayList<String>()
    private var checkoutGate: CompletableDeferred<Unit>? = null

    @Volatile
    private var checkedOut = false

    private fun store(checkoutStatus: HttpStatusCode, checkoutBody: String): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val nodes = "[${node(Atlas, "atlas")},${node(Willow, "willow")}]"
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/checkout") -> {
                    checkouts += String(request.body.toByteArray())
                    checkoutGate?.await()
                    checkedOut = checkoutStatus == HttpStatusCode.Created
                    respond(checkoutBody, checkoutStatus, json)
                }
                path.endsWith("/conversations") && request.method == HttpMethod.Post -> {
                    bodies += String(request.body.toByteArray())
                    respond(CreatedJson, HttpStatusCode.Created, json)
                }
                path.endsWith("/projects") -> respond(if (checkedOut) "[$HarbourOnAtlas,$HarbourOnWillow]" else "[$HarbourOnAtlas]", HttpStatusCode.OK, json)
                path.endsWith("/nodes/any") -> {
                    previewProjects += request.url.parameters["project_id"].orEmpty()
                    respond("""{"node_id":"$Atlas","name":"atlas"}""", HttpStatusCode.OK, json)
                }
                path.endsWith("/nodes") -> respond(nodes, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond(RecentJson, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.refreshProjects() }

        return store
    }

    private fun show(store: Store) {
        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                CompositionLocalProvider(LocalTouch provides false) {
                    NewConversationScreen(store, state, onCreated = {})
                }
            }
        }
    }

    @Test
    fun aNodeWithoutTheProjectOffersToCloneItAndStartsThereOnceSetUp() {
        show(store(HttpStatusCode.Created, HarbourOnWillow))

        rule.onNodeWithContentDescription("Place: atlas, existing checkout").performClick()
        rule.onNodeWithText("Not set up yet, clones harbour").assertExists()
        rule.onNodeWithText("willow").performClick()

        rule.waitUntil(5_000) { runCatching { rule.onNodeWithContentDescription("Place: willow, existing checkout").assertExists() }.isSuccess }
        rule.onRoot().performKeyInput { pressKey(Key.Escape) }
        rule.onNodeWithContentDescription("Message").performTextInput("Restore the copy")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { bodies.isNotEmpty() }
        assertTrue(bodies.single().contains(""""node_id":"$Willow""""))
        assertTrue(bodies.single().contains(""""project_id":"p2""""))
    }

    @Test
    fun aFailedSetupSaysWhatWentWrongAndOffersRetryWithWaysToRecover() {
        show(store(HttpStatusCode.BadRequest, """{"error":"clone_failed","message":"Could not clone example-org/harbour. Check the forge token."}"""))

        rule.onNodeWithContentDescription("Place: atlas, existing checkout").performClick()
        rule.onNodeWithText("willow").performClick()

        rule.waitUntil(5_000) { runCatching { rule.onNodeWithText("Setup failed on willow").assertExists() }.isSuccess }
        rule.onNodeWithText("Retry setup").assertExists()
        rule.onNodeWithContentDescription("More ways to retry setup").performClick()
        rule.onNodeWithText("Show the full log").assertExists()
        rule.onNodeWithText("Start on Any node instead").assertExists()
        rule.onNodeWithText("Remove the checkout").assertDoesNotExist()
    }

    @Test
    fun choosingAnyNodeStartsWithoutAnExplicitNode() {
        show(store(HttpStatusCode.Created, HarbourOnWillow))

        rule.onNodeWithContentDescription("Place: atlas, existing checkout").performClick()
        rule.waitUntil(5_000) { previewProjects.isNotEmpty() }
        assertEquals("p1", previewProjects.last())
        rule.onNodeWithText("Any node").performClick()
        rule.onRoot().performKeyInput { pressKey(Key.Escape) }
        rule.onNodeWithContentDescription("Message").performTextInput("Restore the copy")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { bodies.isNotEmpty() }
        assertEquals("00000000-0000-0000-0000-000000000000", Regex(""""node_id":"([^"]+)"""").find(bodies.single())?.groupValues?.get(1))
    }

    @Test
    fun sendingDuringSetupWaitsThenStartsOnTheNewCheckout() {
        val gate = CompletableDeferred<Unit>()
        checkoutGate = gate
        show(store(HttpStatusCode.Created, HarbourOnWillow))

        rule.onNodeWithContentDescription("Place: atlas, existing checkout").performClick()
        rule.onNodeWithText("willow").performClick()
        rule.waitUntil(5_000) { checkouts.isNotEmpty() }
        rule.onNodeWithText("Cloning harbour on willow").assertExists()
        rule.onNodeWithContentDescription("Message").performTextInput("Restore the copy")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }
        rule.runOnIdle { assertTrue(bodies.isEmpty()) }

        gate.complete(Unit)

        rule.waitUntil(5_000) { bodies.isNotEmpty() }
        assertTrue(bodies.single().contains("\"node_id\":\"$Willow\""))
        assertTrue(bodies.single().contains("\"project_id\":\"p2\""))
        assertTrue(bodies.single().contains("\"prompt\":\"Restore the copy\""))
    }

    @Test
    fun stoppingWaitingKeepsTheDraftAndDoesNotStartAConversation() {
        val gate = CompletableDeferred<Unit>()
        checkoutGate = gate
        show(store(HttpStatusCode.Created, HarbourOnWillow))

        rule.onNodeWithContentDescription("Place: atlas, existing checkout").performClick()
        rule.onNodeWithText("willow").performClick()
        rule.waitUntil(5_000) { checkouts.isNotEmpty() }
        rule.onNodeWithContentDescription("Message").performTextInput("Keep this draft")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }
        rule.onNodeWithText("Stop waiting").performClick()
        gate.complete(Unit)

        rule.onNodeWithContentDescription("Place: atlas, existing checkout").assertExists()
        rule.onNodeWithText("Keep this draft").assertExists()
        rule.runOnIdle { assertTrue(bodies.isEmpty()) }
    }

    @Test
    fun aFailedSetupDoesNotStartOnTheOriginalNodeAndCanBeRetried() {
        show(store(HttpStatusCode.BadRequest, """{"error":"clone_failed","message":"Check the forge sign-in."}"""))

        rule.onNodeWithContentDescription("Place: atlas, existing checkout").performClick()
        rule.onNodeWithText("willow").performClick()
        rule.waitUntil(5_000) { runCatching { rule.onNodeWithText("Setup failed on willow").assertExists() }.isSuccess }
        rule.onNodeWithContentDescription("Message").performTextInput("Keep this draft")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }
        rule.runOnIdle { assertTrue(bodies.isEmpty()) }
        rule.onNodeWithText("Retry setup").performClick()

        rule.waitUntil(5_000) { checkouts.size == 2 }
        rule.onNodeWithText("Keep this draft").assertExists()
    }
}
