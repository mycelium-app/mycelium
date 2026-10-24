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
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.graphics.asSkiaBitmap
import java.io.File
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

private fun artifact(id: String, kind: String, title: String, owner: String, isPublic: Boolean = false, url: String? = null, thumbnailVersion: Int? = null) =
    """{"id":"$id","owner":"$owner","conversation_id":null,"kind":"$kind","title":"$title","slug":"$id-ab12","public":$isPublic,"latest_version":2,"thumbnail_version":${thumbnailVersion ?: "null"},"url":${url?.let { "\"$it\"" } ?: "null"},
"created_at":"2026-10-22T09:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private val Artifacts = "[" + listOf(
    artifact("a1", "design", "Landing sketch", "u1", url = "https://landing-sketch-ab12.artifacts.test"),
    artifact("a2", "page", "Planning board", "u1"),
    artifact("a3", "markdown", "Release notes", "u2", isPublic = true),
).joinToString(",") + "]"

private val TinyPng = java.util.Base64.getDecoder().decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")

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
    private val opened = mutableListOf<String>()

    private fun showArtifacts(list: String = Artifacts, accessRequests: String = "[]", links: String = "[]") {
        var linked = true
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += Triple(request.method.value, path, String(request.body.toByteArray()))

            when {
                request.method == HttpMethod.Patch -> respond(artifact("a1", "design", "Landing sketch", "u1", isPublic = true), HttpStatusCode.OK, json)
                request.method == HttpMethod.Delete -> { linked = false; respond("", HttpStatusCode.NoContent) }
                path.endsWith("/artifact-access-requests") -> respond(accessRequests, HttpStatusCode.OK, json)
                path.endsWith("/approve") || path.endsWith("/decline") -> respond(accessRequests.trim('[', ']').replace("requested", if (path.endsWith("/approve")) "approved" else "declined"), HttpStatusCode.OK, json)
                path.endsWith("/links") -> respond(if (linked) links else "[]", HttpStatusCode.OK, json)
                path.endsWith("/thumbnail") -> respond(TinyPng, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "image/png"))
                path.endsWith("/artifacts") -> respond(list, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                ArtifactsScreen(store, state, Me("u1", "person@example.test", "Person"), platform, onOpen = { opened += it })
            }
        }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Edited", substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun shown(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun sent(method: String, suffix: String) = requests.toList().firstOrNull { it.first == method && it.second.endsWith(suffix) }

    @Test
    fun artefactAccessCanBeRemovedFromTheCardMenu() {
        showArtifacts(links = """[{"id":"l1","artifact_id":"a2","conversation_id":"c1","permission":"view"}]""")
        rule.onNodeWithContentDescription("More for Planning board").performClick()
        rule.onNodeWithText("Manage access").performClick()
        rule.waitUntil(5_000) { shown("Conversation c1") }
        File("/tmp/artefact-access-actual.png").writeBytes(org.jetbrains.skia.Image.makeFromBitmap(rule.onAllNodes(isRoot()).get(1).captureToImage().asSkiaBitmap()).encodeToData()!!.bytes)
        rule.onNodeWithContentDescription("Remove access for Conversation c1").performClick()
        rule.waitUntil(5_000) { sent("DELETE", "/artifacts/a2/links/l1") != null && !shown("Conversation c1") }
    }

    @Test
    fun approvingAnArtefactAccessRequestRemovesThePendingCard() {
        showArtifacts(accessRequests = """[{"id":"r1","artifact_id":"a2","conversation_id":"c1","permission":"view","reason":"Continue the plan","state":"requested","created_at":"2026-10-24T10:00:00Z"}]""")
        rule.waitUntil(5_000) { shown("Continue the plan") }
        rule.onNodeWithText("Approve").performClick()
        rule.waitUntil(5_000) { sent("POST", "/artifact-access-requests/r1/approve") != null && !shown("Continue the plan") }
    }

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

        rule.onNodeWithText("Delete artefact").performClick()

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
    fun clickingACardOpensItsFullView() {
        showArtifacts()

        rule.onNodeWithContentDescription("Open Landing sketch").performClick()
        rule.onNodeWithContentDescription("Open Planning board").performClick()

        assertEquals(listOf("a1", "a2"), opened)
        assertTrue(platform.navigated.isEmpty())
    }

    @Test
    fun theOpenMenuItemOpensTheFullViewAndOpenInBrowserNavigates() {
        showArtifacts()

        rule.onNodeWithContentDescription("More for Planning board").performClick()
        rule.onNodeWithText("Open").performClick()
        rule.onNodeWithContentDescription("More for Planning board").performClick()
        rule.onNodeWithText("Open in browser").performClick()

        assertEquals(listOf("a2"), opened)
        assertEquals(listOf("https://mycelium.test/api/v1/artifacts/a2/versions/2/content"), platform.navigated)
    }

    @Test
    fun copyLinkCopiesTheArtifactUrlWhenItHasOne() {
        showArtifacts()

        rule.onNodeWithContentDescription("More for Landing sketch").performClick()
        rule.onNodeWithText("Copy link").performClick()

        assertEquals(listOf("https://landing-sketch-ab12.artifacts.test"), platform.copied)
    }

    @Test
    fun aPublicArtifactAlsoOffersItsPrivateLink() {
        showArtifacts()

        rule.onNodeWithContentDescription("More for Release notes").performClick()
        rule.onNodeWithText("Copy private link").performClick()

        assertEquals(listOf("https://mycelium.test/api/v1/artifacts/a3/versions/2/content"), platform.copied)
    }

    @Test
    fun aPrivateArtifactHasNoSeparatePrivateLink() {
        showArtifacts()

        rule.onNodeWithContentDescription("More for Landing sketch").performClick()

        rule.onNodeWithText("Copy private link").assertDoesNotExist()
    }

    @Test
    fun aSnapshotIsRequestedOnceAndTheCardStillOpens() {
        showArtifacts("[" + artifact("a2", "page", "Planning board", "u1", thumbnailVersion = 1) + "]")

        rule.waitUntil(5_000) { sent("GET", "/artifacts/a2/thumbnail") != null }
        rule.onNodeWithContentDescription("Open Planning board").performClick()

        assertEquals(1, requests.toList().count { it.second.endsWith("/artifacts/a2/thumbnail") })
        assertEquals(listOf("a2"), opened)
    }

    @Test
    fun noSnapshotIsRequestedWhenTheArtifactHasNone() {
        showArtifacts("[" + artifact("a2", "page", "Planning board", "u1") + "]")

        rule.waitForIdle()

        assertEquals(0, requests.toList().count { it.second.endsWith("/thumbnail") })
    }
}
