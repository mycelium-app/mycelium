package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.state.WorkflowAgent
import mycelium.state.WorkflowEnd
import mycelium.state.WorkflowProgress
import mycelium.ui.ArtifactsScreen
import mycelium.ui.LoadFailedState
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue
import kotlin.time.Clock
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant

private val RenderOrigin = """{"conversation_id":"c1","conversation_title":"Bump the UI kit to 0.31.2","project_id":"p1","project_name":"harbour","project_icon":null}"""

private fun renderedArtifact(id: String, title: String, origin: String?) =
    """{"id":"$id","owner":"u1","kind":"markdown","title":"$title","slug":"$id-ab12","public":false,"latest_version":2,${origin?.let { "\"origin\":$it," }.orEmpty()}"created_at":"2026-10-22T09:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

class ShellRenderTest {
    @get:Rule
    val scopes = TestScopes()

    @Test
    fun theWorkflowCardsRenderStoppedFailedAndStalled() {
        val stopped = WorkflowProgress("a", emptyList(), plannedPhases = listOf("Implement", "Verify", "Fix"), end = WorkflowEnd("stopped", Instant.parse("2026-10-27T00:42:10Z")))
        val failedAgents = listOf(WorkflowAgent("a", "Usage card arrow", "Implement", finished = true, tokens = 410_000, turns = 14, elapsedMs = 540_000), WorkflowAgent("b", "Usage history page", "Implement", finished = true, failed = true, tokens = 120_000, turns = 4, elapsedMs = 180_000))
        val failed = WorkflowProgress("b", failedAgents, plannedPhases = listOf("Implement", "Verify"), end = WorkflowEnd("failed", reason = "Claude Code's weekly limit was reached and the workflow ended."))
        val stalledAgents = listOf(WorkflowAgent("a", "Activity all time", "Implement", finished = true, tokens = 380_000, turns = 11, elapsedMs = 480_000), WorkflowAgent("b", "Sidebar row menus", "Implement", finished = false))
        val stalled = WorkflowProgress("c", stalledAgents, plannedPhases = listOf("Implement", "Verify"), lastLineAt = Clock.System.now() - 25.minutes)
        val items = listOf(stopped, failed, stalled).mapIndexed { index, progress -> ThreadItem.Workflow("w$index", progress) }

        assertTrue(renderAt("workflow-ended", true, false) { Box(Modifier.width(900.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}, dismissedPins = setOf("workflow:a", "workflow:b", "workflow:c")) } }.length() > 3_000)
    }

    @Test
    fun theArtefactsAndFailedLoadScreensRender() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val body = if (request.url.encodedPath.endsWith("/artifacts")) "[" + listOf(renderedArtifact("a1", "Release notes v0.11.0", RenderOrigin), renderedArtifact("a2", "Loose notes", null)).joinToString(",") + "]" else "[]"

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refreshArtifacts() }

        assertTrue(renderAt("artefacts", true, false) { ArtifactsScreen(store, store.state.value, Me("u1", "sam@example.com", "Sam"), OpeningPlatform()) }.length() > 3_000)
        assertTrue(renderAt("load-failed", true, false) { Box(Modifier.fillMaxSize()) { LoadFailedState("tasks", {}, Modifier.fillMaxSize()) } }.length() > 1_000)
    }
}
