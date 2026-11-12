package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewConversationScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val Atlas = "aaaaaaaa-0000-0000-0000-000000000001"
private const val Orion = "aaaaaaaa-0000-0000-0000-000000000002"
private const val Willow = "aaaaaaaa-0000-0000-0000-000000000003"

private fun node(id: String, name: String) = """{"id":"$id","name":"$name","hostname":"$name","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[],"harness_models":[]}"""

private fun backups(vararg nodes: Pair<String, String>) = """{"id":"p1","node_id":"$Atlas","name":"backups","path":"/home/sam/projects/acme-org/backups","forge_id":"f1","repository":"acme-org/backups",
"checkouts":[${nodes.joinToString(",") { (node, path) -> """{"id":"k-$node","node_id":"$node","path":"$path","created_at":"2026-10-22T10:00:00Z"}""" }}],"created_at":"2026-10-22T10:00:00Z"}"""

private val OnAtlasAndOrion = backups(Atlas to "/home/sam/projects/acme-org/backups", Orion to "/srv/backups")

private val OnAllThree = backups(Atlas to "/home/sam/projects/acme-org/backups", Orion to "/srv/backups", Willow to "/home/sam/backups")

private const val Conversations = """[
{"id":"c1","owner":"u","node_id":"$Atlas","agent":"claude","cwd":"/home/sam/projects/acme-org/backups","title":"Prune snapshots","status":"done","project_id":"p1",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T11:00:00Z","archived_at":null},
{"id":"c2","owner":"u","node_id":"$Orion","agent":"claude","cwd":"/srv/backups","title":"Restore drill","status":"done","project_id":"p1",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T12:00:00Z","archived_at":null}]"""

class NewConversationCheckoutsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val checkouts = CopyOnWriteArrayList<String>()
    @Volatile
    private var cloned = false

    private val conversationBodies = CopyOnWriteArrayList<String>()

    private fun store(project: String = OnAtlasAndOrion): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val nodes = "[${node(Atlas, "atlas")},${node(Orion, "orion")},${node(Willow, "willow")}]"
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/checkout") -> {
                    checkouts += String(request.body.toByteArray())
                    cloned = true
                    respond(OnAllThree, HttpStatusCode.OK, json)
                }
                path.endsWith("/conversations") && request.method == HttpMethod.Post -> {
                    conversationBodies += String(request.body.toByteArray())
                    respond(Conversations.removePrefix("[").substringBefore("},\n{").plus("}"), HttpStatusCode.Created, json)
                }
                path.endsWith("/projects") -> respond("[${if (cloned) OnAllThree else project}]", HttpStatusCode.OK, json)
                path.endsWith("/nodes/any") -> respond("""{"node_id":"$Atlas","name":"atlas"}""", HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(nodes, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond(Conversations, HttpStatusCode.OK, json)
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
    fun aProjectOnSeveralNodesRunsOnTheNodeOfItsLastConversation() {
        show(store())

        rule.onNodeWithContentDescription("Run on orion").assertExists()
    }

    @Test
    fun theRunOnMenuListsTheCheckoutsWithTheLastUsedNodeMarkedAndClonesOntoTheRest() {
        show(store())

        rule.onNodeWithContentDescription("Run on orion").performClick()

        rule.onNodeWithText("Checkouts of backups").assertExists()
        rule.onNodeWithText("Last used").assertExists()
        rule.onNodeWithText("atlas").assertExists()
        rule.onNodeWithText("Clone onto willow").assertExists()
        rule.onAllNodesWithText("Clone onto orion").assertCountEquals(0)
        rule.onAllNodesWithText("Clone onto atlas").assertCountEquals(0)
    }

    @Test
    fun choosingAnotherCheckoutMovesTheRunOnNodeWithoutChangingTheProject() {
        show(store())

        rule.onNodeWithContentDescription("Run on orion").performClick()
        rule.onNodeWithText("atlas").performClick()

        rule.onNodeWithContentDescription("Run on atlas").assertExists()
        rule.onNodeWithContentDescription("Project: backups").assertExists()
    }

    @Test
    fun cloningOntoANodeAddsACheckoutOfTheSameProjectAndRunsThere() {
        show(store())

        rule.onNodeWithContentDescription("Run on orion").performClick()
        rule.onNodeWithText("Clone onto willow").performClick()

        rule.waitUntil(5_000) { runCatching { rule.onNodeWithContentDescription("Run on willow").assertExists() }.isSuccess }
        assertEquals("""{"node_id":"$Willow"}""", checkouts.single())
        rule.onNodeWithContentDescription("Project: backups").assertExists()
    }

    @Test
    fun anOlderServersProjectWithoutCheckoutsStillRunsOnItsNode() {
        show(store("""{"id":"p1","node_id":"$Atlas","name":"backups","path":"/home/sam/projects/acme-org/backups","forge_id":"f1","repository":"acme-org/backups","created_at":"2026-10-22T10:00:00Z"}"""))

        rule.onNodeWithContentDescription("Run on atlas").assertExists()
    }
}
