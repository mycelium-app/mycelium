package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.api.Walkthrough
import mycelium.state.Store
import mycelium.ui.ReflectionsTab
import mycelium.ui.WalkthroughCheckStep
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private val Reflection = """{"id":"r1","week":"2026-10-19","done":"Shipped the importer.\n\n- Fixed the retry loop","learned":"Lifetimes finally make sense.","waiting":"Review from Sam","next_week":"Plan the release","edited":["learned"],"drafted_at":"2026-10-23T16:00:00Z"}"""
private const val Checked = """{"id":"w1","conversation_id":"c1","created_at":"2026-10-23T10:00:00Z","steps":[{"title":"One","explanation":"","files":[]},{"title":"Two","explanation":"","files":[]}],"checks":[{"step":2,"question":"Where does the title come from?","choices":["A literal","The strings file","The server"]}]}"""

class LearningRenderTest {
    @get:Rule
    val scopes = TestScopes()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val reply = when {
                path.endsWith("/reflections") -> "[$Reflection]"
                path.endsWith("/answers") -> """[{"step":2,"choice":0,"correct":false,"explanation":"Step 2 reads it from strings."}]"""
                else -> "{}"
            }

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.learning.load() }

        return store
    }

    @Test
    fun theReflectionsTabAndTheCheckStepRenderAtWideAndNarrowWidths() {
        val walkthrough = kotlinx.serialization.json.Json { ignoreUnknownKeys = true }.decodeFromString<Walkthrough>(Checked)

        for (dark in listOf(true, false)) {
            for (phone in listOf(false, true)) {
                val store = store()

                assertTrue(renderAt("reflections", dark, phone) { Box(Modifier.fillMaxSize()) { ReflectionsTab(store, OpeningPlatform()) {} } }.length() > 2_000)
                assertTrue(renderAt("walkthrough-check", dark, phone) { Box(Modifier.fillMaxSize()) { WalkthroughCheckStep(store, walkthrough) {} } }.length() > 2_000)
            }
        }
    }
}
