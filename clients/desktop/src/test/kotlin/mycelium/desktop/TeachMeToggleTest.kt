package mycelium.desktop

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.input.key.Key
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private fun conversation(teachMe: Boolean) =
    """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Learn lifetimes","status":"done","acp_session_id":"s1","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","teach_me":$teachMe}"""

private const val Dividers = """[{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{"text":"go"}},{"conversation_id":"c1","seq":2,"kind":"teach_mode","payload":{"on":true}},{"conversation_id":"c1","seq":3,"kind":"teach_mode","payload":{"on":false}}]"""

@OptIn(ExperimentalTestApi::class)
class TeachMeToggleTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val patches = CopyOnWriteArrayList<String>()

    private fun show(teachMe: Boolean) {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Patch && path.endsWith("/conversations/c1")) {
                patches += String(request.body.toByteArray())
            }

            val body = when {
                path.endsWith("/conversations") -> "[${conversation(teachMe)}]"
                path.endsWith("/conversations/c1") -> conversation(!teachMe)
                path.endsWith("/events") -> Dividers
                path.endsWith("/queue") || path.endsWith("/nodes") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Teach me").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theTeachMeButtonReportsItsStateAndTurnsItOnWhenPressed() {
        show(teachMe = false)

        rule.onNodeWithContentDescription("Teach me").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Off")).performClick()

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertEquals("""{"teach_me":true}""", patches.single())
    }

    @Test
    fun theTeachMeButtonTurnsItOffWhileItIsOn() {
        show(teachMe = true)

        rule.onNodeWithContentDescription("Teach me").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "On")).performClick()

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertEquals("""{"teach_me":false}""", patches.single())
    }

    @Test
    fun controlShiftTInTheMessageFieldTogglesTeachMe() {
        show(teachMe = false)

        rule.onNode(hasSetTextAction()).performClick()
        rule.onNode(hasSetTextAction()).performKeyInput {
            keyDown(Key.CtrlLeft)
            keyDown(Key.ShiftLeft)
            pressKey(Key.T)
            keyUp(Key.ShiftLeft)
            keyUp(Key.CtrlLeft)
        }

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertEquals("""{"teach_me":true}""", patches.single())
    }

    @Test
    fun theThreadShowsADividerEachTimeTeachMeChanges() {
        show(teachMe = false)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Teach me on").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Teach me off").assertExists()
    }
}
