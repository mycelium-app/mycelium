package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import kotlinx.coroutines.runBlocking
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import mycelium.api.Artifact
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import mycelium.ui.ArtifactsScreen
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewArtifactDialog
import org.junit.Rule
import org.junit.Test
import kotlin.test.assertEquals

private const val CreatedSlides = """{"id":"a1","owner":"u1","kind":"slides","title":"Restore drill debrief","slug":"restore-drill-debrief","public":false,"latest_version":1,"created_at":"2026-10-28T09:00:00Z","updated_at":"2026-10-28T09:00:00Z"}"""

class NewArtifactTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            if (request.method == HttpMethod.Post) {
                posts += request.url.encodedPath to String(request.body.toByteArray())
            }

            respond(CreatedSlides, HttpStatusCode.Created, json)
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    @Test
    fun creatingWithoutANameNamesTheKindInTheErrorAndSendsNothing() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { NewArtifactDialog(store, emptyList(), emptyList(), onCreated = {}, onDismiss = {}) } }

        rule.onNodeWithContentDescription("Design system. Tokens, type and components").performClick()
        rule.onNodeWithText("Create").performClick()

        rule.onNodeWithText("There is a problem").assertExists()
        assertEquals(true, rule.onAllNodesWithText("Enter a name for the design system").fetchSemanticsNodes().isNotEmpty())
        assertEquals(emptyList(), posts.toList())
    }

    @Test
    fun creatingASlidesDeckSendsItsKindAndNameAndOpensIt() {
        val store = store()
        var opened: Artifact? = null
        rule.setContent { MyceliumTheme(true) { NewArtifactDialog(store, emptyList(), emptyList(), onCreated = { opened = it }, onDismiss = {}) } }

        rule.onNodeWithContentDescription("Slides. A deck to present").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Restore drill debrief")
        rule.onNodeWithText("Create").performClick()
        rule.waitUntil(5_000) { opened != null }

        assertEquals("/api/v1/artifacts", posts.single().first)
        assertEquals(true, posts.single().second.contains(""""kind":"slides"""") && posts.single().second.contains(""""title":"Restore drill debrief""""))
        assertEquals("a1", opened?.id)
    }

    private fun deck(id: String, kind: String, title: String) =
        """{"id":"$id","owner":"u1","kind":"$kind","title":"$title","slug":"$id","public":false,"latest_version":1,"created_at":"2026-10-28T09:00:00Z","updated_at":"2026-10-28T09:00:00Z"}"""

    @Test
    fun theKindMenuNamesThePickedKindAndShowsOnlyThatKind() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val body = if (request.url.encodedPath.endsWith("/artifacts")) "[" + deck("a1", "slides", "Restore drill debrief") + "," + deck("a2", "dashboard", "Backup health") + "]" else "[]"

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ArtifactsScreen(store, state, Me("u1", "p@example.test", "Person"), QuietPlatform) } }

        rule.onNodeWithText("Backup health").assertExists()
        rule.onNodeWithContentDescription("Kind: All kinds").performClick()
        rule.onAllNodesWithText("Slides").onLast().performClick()

        rule.onNodeWithContentDescription("Kind: Slides").assertExists()
        rule.onNodeWithText("Restore drill debrief").assertExists()
        assertEquals(0, rule.onAllNodesWithText("Backup health").fetchSemanticsNodes().size)
    }
}
