package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isSelectable
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ArtifactsScreen
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

private fun artifact(id: String, kind: String, title: String, owner: String, isPublic: Boolean = false, url: String? = null) =
    """{"id":"$id","owner":"$owner","conversation_id":null,"kind":"$kind","title":"$title","slug":"$id-ab12","public":$isPublic,"latest_version":2,"url":${url?.let { "\"$it\"" } ?: "null"},
"created_at":"2026-10-22T09:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private val Artifacts = "[" + listOf(
    artifact("a1", "design", "Landing sketch", "u1", url = "https://landing-sketch-ab12.artifacts.test"),
    artifact("a2", "page", "Planning board", "u1"),
    artifact("a3", "markdown", "Release notes", "u2", isPublic = true),
).joinToString(",") + "]"

private class ArtifactsPlatform : Platform {
    val navigated = mutableListOf<String>()
    val copied = mutableListOf<String>()

    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) { navigated += url }
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) { copied += text }
}

class ArtifactsScreenTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<Triple<String, String, String>>())
    private val platform = ArtifactsPlatform()

    private fun showArtifacts() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += Triple(request.method.value, path, String(request.body.toByteArray()))

            when {
                request.method == HttpMethod.Patch -> respond(artifact("a1", "design", "Landing sketch", "u1", isPublic = true), HttpStatusCode.OK, json)
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                path.endsWith("/artifacts") -> respond(Artifacts, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                ArtifactsScreen(store, state, Me("u1", "person@example.test", "Person"), platform)
            }
        }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Landing sketch").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun shown(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun sent(method: String, suffix: String) = requests.toList().firstOrNull { it.first == method && it.second.endsWith(suffix) }

    @Test
    fun theScreenListsEveryArtifactWithItsEditTime() {
        showArtifacts()

        assertTrue(shown("Planning board"))
        assertTrue(shown("Release notes"))
        assertEquals(3, rule.onAllNodesWithText("Edited", substring = true).fetchSemanticsNodes().size)
    }

    @Test
    fun theYoursTabHidesArtifactsOwnedByOthers() {
        showArtifacts()

        rule.onNode(hasText("Yours") and isSelectable()).performClick()

        assertTrue(shown("Planning board"))
        assertFalse(shown("Release notes"))
    }

    @Test
    fun theKindFilterKeepsOnlyThatKind() {
        showArtifacts()

        rule.onNode(hasText("Page") and isSelectable()).performClick()

        assertTrue(shown("Planning board"))
        assertFalse(shown("Landing sketch"))
        assertFalse(shown("Release notes"))
    }

    @Test
    fun searchingNarrowsTheArtifactsByTitle() {
        showArtifacts()

        rule.onNode(hasSetTextAction()).performTextInput("notes")

        rule.waitUntil(5_000) { !shown("Landing sketch") }
        assertTrue(shown("Release notes"))
    }

    @Test
    fun makePublicPatchesTheArtifactAsPublic() {
        showArtifacts()

        rule.onNodeWithContentDescription("More for Planning board").performClick()
        rule.onNodeWithText("Make public").performClick()

        rule.waitUntil(5_000) { sent("PATCH", "/artifacts/a2") != null }
        assertEquals("""{"public":true}""", sent("PATCH", "/artifacts/a2")?.third)
    }

    @Test
    fun anArtifactOwnedByAnotherPersonOffersOnlyOpenAndCopyLink() {
        showArtifacts()

        rule.onNodeWithContentDescription("More for Release notes").performClick()

        assertTrue(shown("Copy link"))
        assertFalse(shown("Make private"))
        assertFalse(shown("Delete"))
    }

    @Test
    fun deleteAsksForConfirmationBeforeRemovingTheArtifact() {
        showArtifacts()

        rule.onNodeWithContentDescription("More for Planning board").performClick()
        rule.onNodeWithText("Delete").performClick()

        rule.onNodeWithText("Delete Planning board?").assertExists()
        assertNull(sent("DELETE", "/artifacts/a2"))

        rule.onNodeWithText("Delete artifact").performClick()

        rule.waitUntil(5_000) { sent("DELETE", "/artifacts/a2") != null }
        rule.waitUntil(5_000) { !shown("Planning board") }
    }

    @Test
    fun cancellingTheDeleteKeepsTheArtifact() {
        showArtifacts()

        rule.onNodeWithContentDescription("More for Planning board").performClick()
        rule.onNodeWithText("Delete").performClick()
        rule.onNodeWithText("Cancel").performClick()

        assertNull(sent("DELETE", "/artifacts/a2"))
        assertTrue(shown("Planning board"))
    }

    @Test
    fun openingADesignGoesToTheEditorAndOtherKindsToTheirContent() {
        showArtifacts()

        rule.onNodeWithContentDescription("Open Landing sketch").performClick()
        rule.onNodeWithContentDescription("Open Planning board").performClick()

        assertEquals(listOf("https://mycelium.test/design/a1", "https://mycelium.test/api/v1/artifacts/a2/versions/2/content"), platform.navigated)
    }

    @Test
    fun copyLinkCopiesTheArtifactUrlWhenItHasOne() {
        showArtifacts()

        rule.onNodeWithContentDescription("More for Landing sketch").performClick()
        rule.onNodeWithText("Copy link").performClick()

        assertEquals(listOf("https://landing-sketch-ab12.artifacts.test"), platform.copied)
    }
}
