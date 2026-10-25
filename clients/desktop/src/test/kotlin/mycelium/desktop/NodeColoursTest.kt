package mycelium.desktop

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.OutgoingContent
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val RoseNodeJson = """[{"id":"n1","name":"laptop","colour":"rose","hostname":"laptop","os":"linux","arch":"x86_64","version":"v0.1.13",
"online":true,"draining":false,"agents":[],"logins":[]}]"""

private object ColoursPlatform : Platform {
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

class NodeColoursTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<HttpRequestData>()

    private fun showNodes() {
        val engine = MockEngine { request ->
            requests += request
            val body = when {
                request.url.encodedPath.endsWith("/nodes") -> RoseNodeJson
                request.url.encodedPath.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, ColoursPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Nodes) } }
    }

    @Test
    fun theNodeMenuShowsTheChosenColourAndSavesANewOne() {
        showNodes()

        rule.onNodeWithContentDescription("More for laptop").performClick()
        rule.onNodeWithContentDescription("Use the rose colour").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Chosen"))
        rule.onNodeWithContentDescription("Use the green colour").performClick()
        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Patch } }

        val saved = requests.last { it.method == HttpMethod.Patch }

        assertEquals("/api/v1/nodes/n1", saved.url.encodedPath)
        assertEquals("""{"colour":"green"}""", (saved.body as OutgoingContent.ByteArrayContent).bytes().decodeToString())
    }
}
