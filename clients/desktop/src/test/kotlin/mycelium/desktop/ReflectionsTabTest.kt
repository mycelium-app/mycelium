package mycelium.desktop

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
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
import mycelium.ui.MyceliumTheme
import mycelium.ui.WellnessScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private fun reflectionJson(learned: String, edited: String) =
    """{"id":"r1","week":"2026-10-19","done":"Shipped the importer","learned":"$learned","waiting":"Review from Sam","next_week":"Plan the release","edited":$edited,"drafted_at":"2026-10-23T16:00:00Z"}"""

@OptIn(ExperimentalTestApi::class)
class ReflectionsTabTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val sent = CopyOnWriteArrayList<Triple<String, String, String>>()
    private val opened = CopyOnWriteArrayList<String>()
    private var reflections = "[]"

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = String(request.body.toByteArray())

            if (request.method != HttpMethod.Get) {
                sent += Triple(request.method.value, path, body)
            }

            val reply = when {
                path.endsWith("/assistants/builtin/wellness") -> """{"id":"w1","name":"Wellness","description":"","state":"idle","builtin_key":"wellness","created_at":"2026-10-01T00:00:00Z"}"""
                path.endsWith("/redraft") -> """{"conversation_id":"c9"}"""
                path.endsWith("/reflections/r1") -> reflectionJson("Mine", """["learned"]""")
                path.endsWith("/reflections") -> reflections
                path.endsWith("/conversations") -> "[]"
                path.endsWith("/wellness/settings") -> WellnessSettingsJson
                path.endsWith("/wellness/overview") -> WellnessOverviewJson
                path.endsWith("/nodes") -> """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.1.10","online":true,"draining":false,"agents":[],"logins":[]}]"""
                else -> "{}"
            }

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { WellnessScreen(store, OpeningPlatform(), onOpenAssistant = { opened += it }) } }
        rule.onNodeWithText("Reflections").performClick()
    }

    @Test
    fun theTabOpensTheWellnessAssistantInsteadOfOwningASchedule() {
        show()
        rule.onNodeWithText("Open the Wellness assistant").performClick()

        rule.waitUntil(5_000) { opened.isNotEmpty() }
        assertEquals(listOf("w1"), opened.toList())
        assertEquals(emptyList(), sent.toList())
    }

    @Test
    fun editingASectionSavesItOnLeavingTheFieldAndMarksItEdited() {
        reflections = "[${reflectionJson("Lifetimes", "[]")}]"
        show()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Week of 19 October 2026").fetchSemanticsNodes().isNotEmpty() }
        val field = hasContentDescription("Learned, week of 19 October 2026")

        rule.onNode(field).performTextClearance()
        rule.onNode(field).performTextInput("Mine")
        rule.onNodeWithContentDescription("Redraft the week of 19 October 2026").performClick()

        rule.waitUntil(5_000) { sent.any { it.second.endsWith("/redraft") } }
        assertEquals(Triple("PATCH", "/api/v1/reflections/r1", """{"learned":"Mine"}"""), sent.first())
        assertEquals("POST", sent.last().first)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Edited").fetchSemanticsNodes().isNotEmpty() }
    }
}
