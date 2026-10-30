package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.Issue
import mycelium.api.MyceliumApi
import mycelium.api.PullRequestComment
import mycelium.api.PullRequestItem
import mycelium.api.PullRequestState
import mycelium.api.StreamMessage
import mycelium.api.parseStreamMessage
import mycelium.state.Store
import mycelium.state.AppState
import mycelium.state.ForgeReference
import mycelium.state.PasteDisplay
import mycelium.state.expandedPastes
import mycelium.state.serialisedPastes
import mycelium.state.conversationPullRequestText
import mycelium.state.forgeReferences
import mycelium.state.pullRequestFileDiffs
import mycelium.state.reduce
import mycelium.state.unresolvedOnly
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

private fun issue(number: Long, title: String, updated: String?) = Issue("f1", "acme-org/acme-backups", number, title, "https://x/issues/$number", updatedAt = updated)

private fun request(number: Long, title: String, updated: String?) =
    PullRequestItem("f1", "acme-org/acme-backups", number, "PR #$number", title, "https://x/pull/$number", state = PullRequestState.Open, updatedAt = updated)

class ForgeItemsTest {
    @Test
    fun referencesMixIssuesAndPullRequestsNewestFirstAndNarrowByTitleOrNumber() {
        val issues = listOf(issue(412, "Restore fails", "2026-10-01T10:00:00Z"))
        val requests = listOf(request(9, "Restore rewrite", "2026-10-02T10:00:00Z"), request(3, "Docs", "2026-10-03T10:00:00Z"))

        assertEquals(listOf("PR #3", "PR #9", "#412"), forgeReferences(issues, requests, "").map { it.label })
        assertEquals(listOf("PR #9", "#412"), forgeReferences(issues, requests, "restore").map { it.label })
        assertEquals(listOf("#412"), forgeReferences(issues, requests, "#412").map { it.label })
    }

    @Test
    fun theUnresolvedFilterKeepsCommentsThatAreOpenOrCannotBeResolved() {
        val timeline = listOf(
            PullRequestComment(body = "done", resolved = true),
            PullRequestComment(body = "open", resolved = false),
            PullRequestComment(body = "plain", resolved = null),
        )

        assertEquals(listOf("open", "plain"), unresolvedOnly(timeline).map { it.body })
    }

    @Test
    fun aForgeDiffIsSplitIntoOneDiffPerFile() {
        val text = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-x\n+y\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1,2 @@\n k\n+z\n"

        val files = pullRequestFileDiffs(text)

        assertEquals(listOf("a.rs", "b.rs"), files.map { it.path })
        assertEquals(listOf(1, 1), files.map { it.added })
    }

    @Test
    fun aSidebarRowShowsThePullRequestNumberWithItsState() {
        assertEquals("PR #12 merged", conversationPullRequestText("PR #12", "merged"))
        assertEquals("MR !5", conversationPullRequestText("MR !5", null))
        assertEquals(null, conversationPullRequestText(null, null))
    }

    @Test
    fun aForgeChangedMessageBumpsTheRevisionSoListsReload() {
        val message = parseStreamMessage("""{"type":"forge_changed"}""")

        assertEquals(1, reduce(AppState(), assertIs<StreamMessage.ForgeChanged>(message)).forgeRevision)
    }

    @Test
    fun pullRequestsAreListedFromTheControlPlaneWithTheirPeople() = runTest {
        val body = """{"pull_requests":[{"forge_id":"f1","repository":"acme-org/acme-backups","number":9,"label":"PR #9","title":"Newer","url":"https://x/9","state":"merged","draft":false,"source_branch":"topic","target_branch":"main","author":{"name":"samexample"}}],"failures":[]}"""
        val engine = MockEngine { respond(body, headers = headersOf(HttpHeaders.ContentType, "application/json")) }

        val listing = MyceliumApi("https://mycelium.test", null, engine).pullRequests()

        assertEquals(PullRequestState.Merged, listing.pullRequests.single().state)
        assertEquals("samexample", listing.pullRequests.single().author?.name)
    }

    @Test
    fun avatarCachingKeepsDifferentForgeAccountsSeparate() = runTest {
        val fetched = mutableListOf<String>()
        val engine = MockEngine { request ->
            fetched += request.url.encodedPath
            respond(request.url.encodedPath.encodeToByteArray())
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), this)
        val avatar = "https://example.org/avatar"

        assertEquals("/api/v1/forges/f1/avatar", store.forgeAvatarBytes("f1", avatar)?.decodeToString())
        assertEquals("/api/v1/forges/f2/avatar", store.forgeAvatarBytes("f2", avatar)?.decodeToString())
        store.forgeAvatarBytes("f1", avatar)
        assertEquals(listOf("/api/v1/forges/f1/avatar", "/api/v1/forges/f2/avatar"), fetched)
    }

    @Test
    fun referencesAndPastedTextKeepTheirOwnContentWhenSerialisedTogether() {
        val reference = '\uE000'
        val paste = '\uE001'
        val references = mapOf(reference to ForgeReference("PR #9", "Newer change", "https://example.org/pull/9"))
        val pastes = mapOf(paste to "A pasted note")

        assertEquals("PR #9 https://example.org/pull/9 A pasted note", expandedPastes("$reference $paste", pastes, references = references))
        assertEquals(serialisedPastes(paste.toString(), pastes), serialisedPastes(paste.toString(), pastes, references = references))
        assertEquals("PR #9", PasteDisplay("$reference $paste", pastes, references = references).labelOf(reference))
    }

    @Test
    fun citationsReferencesImagesAndPastesKeepTheirOwnLabelsAndContent() {
        val citation = '\uE000'
        val reference = '\uE001'
        val paste = '\uE002'
        val image = '\uE003'
        val citations = mapOf(citation to "The retry delay")
        val references = mapOf(reference to ForgeReference("PR #9", "Restore", "https://example.org/pull/9"))
        val pastes = mapOf(paste to "A pasted note")
        val display = PasteDisplay("$citation $reference $paste $image", pastes, mapOf(image to "sample.png"), citations, references)

        assertEquals("PR #9", display.labelOf(reference))
        assertEquals("\"The retry delay\"", display.labelOf(citation))
        assertEquals("The retry delay PR #9 https://example.org/pull/9 A pasted note $image", expandedPastes("$citation $reference $paste $image", pastes, citations, references))
        assertEquals("\n> The retry delay\n PR #9 https://example.org/pull/9", serialisedPastes("$citation $reference", pastes, citations, references))
    }

    @Test
    fun aReferenceChipShowsItsLabelInTheComposerAndSendsItsLabelAndAddress() {
        val chip = '\uE000'
        val references = mapOf(chip to ForgeReference("PR #9", "Newer change", "https://x/pull/9"))

        assertEquals("PR #9", PasteDisplay("see $chip", emptyMap(), references = references).labelOf(chip))
        assertEquals("see PR #9 https://x/pull/9", serialisedPastes("see $chip", emptyMap(), references = references))
    }
}
