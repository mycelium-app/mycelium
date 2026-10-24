package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonObject
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.StreamMessage
import mycelium.api.parseStreamMessage
import mycelium.state.AppState
import mycelium.state.ArtifactPreview
import mycelium.state.artifactPreview
import mycelium.api.Artifact
import mycelium.state.MyceliumCall
import mycelium.state.MyceliumTool
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.YourArtifacts
import mycelium.state.AllArtifacts
import mycelium.state.artifactOpenUrl
import mycelium.state.myceliumCall
import mycelium.state.myceliumTool
import mycelium.state.reduce
import mycelium.state.visibleArtifacts
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

private val Json = headersOf(HttpHeaders.ContentType, "application/json")

private fun artifactJson(id: String, kind: String = "design", title: String = "Landing", owner: String = "u1", isPublic: Boolean = false, updatedAt: String = "2026-10-22T10:00:00Z", url: String? = null) =
    """{"id":"$id","owner":"$owner","conversation_id":null,"kind":"$kind","title":"$title","slug":"landing-ab12","public":$isPublic,"latest_version":3,"url":${url?.let { "\"$it\"" } ?: "null"},"created_at":"2026-10-22T09:00:00Z","updated_at":"$updatedAt"}"""

private fun call(title: String, input: String?, output: String? = null, status: ToolStatus = ToolStatus.Done) =
    ThreadItem.ToolCall("k", "t1", title, "other", title, status, null, output, input = input?.let { MyceliumJson.parseToJsonElement(it) as JsonObject })

private fun streamed(json: String) = assertIs<StreamMessage.ArtifactChanged>(parseStreamMessage(json))

class ArtifactsTest {
    @Test
    fun anArtifactStreamEventAddsOrReplacesTheArtifact() {
        val created = streamed("""{"type":"artifact",${artifactJson("a1").trim('{', '}')}}""")
        val renamed = streamed("""{"type":"artifact",${artifactJson("a1", title = "Pricing").trim('{', '}')}}""")

        val state = reduce(reduce(AppState(), created), renamed)

        assertEquals(listOf("Pricing"), state.artifacts.values.map { it.title })
        assertEquals(3, state.artifacts.getValue("a1").latestVersion)
    }

    @Test
    fun anArtifactStreamEventWithTheDeletedFlagRemovesIt() {
        val created = streamed("""{"type":"artifact",${artifactJson("a1").trim('{', '}')}}""")
        val deleted = streamed("""{"type":"artifact","deleted":true,${artifactJson("a1").trim('{', '}')}}""")

        assertEquals(emptySet(), reduce(reduce(AppState(), created), deleted).artifacts.keys)
    }

    @Test
    fun aCommentStreamEventLeavesTheArtifactsAlone() {
        val before = reduce(AppState(), streamed("""{"type":"artifact",${artifactJson("a1").trim('{', '}')}}"""))
        val message = parseStreamMessage("""{"type":"artifact_comment","artifact_id":"a1","id":"c1"}""")

        assertEquals(StreamMessage.ArtifactCommentAdded("a1"), message)
        assertEquals(before, reduce(before, message!!))
    }

    @Test
    fun theYoursTabKeepsOnlyTheSignedInUsersArtifactsNewestEditFirst() {
        val artifacts = listOf(
            streamed("""{"type":"artifact",${artifactJson("a1", updatedAt = "2026-10-20T10:00:00Z").trim('{', '}')}}""").artifact,
            streamed("""{"type":"artifact",${artifactJson("a2", owner = "u2", updatedAt = "2026-10-21T10:00:00Z").trim('{', '}')}}""").artifact,
            streamed("""{"type":"artifact",${artifactJson("a3", updatedAt = "2026-10-22T10:00:00Z").trim('{', '}')}}""").artifact,
        )

        assertEquals(listOf("a3", "a2", "a1"), visibleArtifacts(artifacts, AllArtifacts, "u1", null, "").map { it.id })
        assertEquals(listOf("a3", "a1"), visibleArtifacts(artifacts, YourArtifacts, "u1", null, "").map { it.id })
    }

    @Test
    fun theKindFilterAndSearchNarrowTheArtifacts() {
        val artifacts = listOf(
            streamed("""{"type":"artifact",${artifactJson("a1", kind = "page", title = "Planning board").trim('{', '}')}}""").artifact,
            streamed("""{"type":"artifact",${artifactJson("a2", kind = "design", title = "Planning sketch").trim('{', '}')}}""").artifact,
            streamed("""{"type":"artifact",${artifactJson("a3", kind = "design", title = "Logo").trim('{', '}')}}""").artifact,
        )

        assertEquals(setOf("a2", "a3"), visibleArtifacts(artifacts, AllArtifacts, "u1", "design", "").map { it.id }.toSet())
        assertEquals(listOf("a2"), visibleArtifacts(artifacts, AllArtifacts, "u1", "design", " PLANNING ").map { it.id })
    }

    @Test
    fun aDesignOpensInTheEditorAndOtherKindsOpenTheirLatestContent() {
        assertEquals("https://mycelium.test/design/a1", artifactOpenUrl("https://mycelium.test/", "a1", "design", 3))
        assertEquals("https://mycelium.test/api/v1/artifacts/a2/versions/3/content", artifactOpenUrl("https://mycelium.test", "a2", "page", 3))
    }

    @Test
    fun refreshingLoadsTheArtifactsAndRenamingPatchesTheTitle() = runTest {
        val requests = mutableListOf<Triple<String, String, String>>()
        val engine = MockEngine { request ->
            requests += Triple(request.method.value, request.url.encodedPath, String(request.body.toByteArray()))

            when {
                request.method == HttpMethod.Patch -> respond(artifactJson("a1", title = "Pricing"), HttpStatusCode.OK, Json)
                else -> respond("[${artifactJson("a1")}]", HttpStatusCode.OK, Json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), backgroundScope)

        store.refreshArtifacts()
        store.renameArtifact(store.state.value.artifacts.getValue("a1"), "Pricing")

        assertEquals("Pricing", store.state.value.artifacts.getValue("a1").title)
        assertEquals("""{"title":"Pricing"}""", requests.last().third)
    }

    @Test
    fun makingAnArtifactPublicPatchesPublicAndDeletingRemovesIt() = runTest {
        val requests = mutableListOf<Triple<String, String, String>>()
        val engine = MockEngine { request ->
            requests += Triple(request.method.value, request.url.encodedPath, String(request.body.toByteArray()))

            when (request.method) {
                HttpMethod.Patch -> respond(artifactJson("a1", isPublic = true), HttpStatusCode.OK, Json)
                HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                else -> respond("[${artifactJson("a1")}]", HttpStatusCode.OK, Json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), backgroundScope)

        store.refreshArtifacts()
        store.setArtifactPublic(store.state.value.artifacts.getValue("a1"), true)

        assertTrue(store.state.value.artifacts.getValue("a1").isPublic)
        assertEquals("""{"public":true}""", requests.last().third)

        store.deleteArtifact(store.state.value.artifacts.getValue("a1"))

        assertEquals(emptyMap(), store.state.value.artifacts)
        assertEquals("DELETE" to "/api/v1/artifacts/a1", requests.last().let { it.first to it.second })
    }

    @Test
    fun theArtifactToolsAreRecognised() {
        assertEquals(MyceliumTool.CreateArtifact, myceliumTool("mcp__mycelium__create_artifact"))
        assertEquals(MyceliumTool.UpdateArtifact, myceliumTool("mcp__mycelium__update_artifact"))
        assertEquals(MyceliumTool.DesignSetArtboard, myceliumTool("mcp__mycelium__design_set_artboard"))
        assertEquals(MyceliumTool.ReadArtifact, myceliumTool("mcp__mycelium__read_artifact"))
    }

    @Test
    fun createArtifactTakesTheKindTitleIdAndVersionFromTheOutput() {
        val output = """Created design "Pricing page" (artifact 11111111-1111-1111-1111-111111111111, version 1)."""
        val parsed = assertIs<MyceliumCall.CreateArtifact>(myceliumCall(call("mcp__mycelium__create_artifact", """{"kind":"design","title":"Pricing page"}""", output)))

        assertEquals("design", parsed.kind)
        assertEquals("Pricing page", parsed.title)
        assertEquals("11111111-1111-1111-1111-111111111111", parsed.artifactId)
        assertEquals(1, parsed.version)
    }

    @Test
    fun createArtifactWhileRunningHasTheTitleButNoIdYet() {
        val parsed = assertIs<MyceliumCall.CreateArtifact>(myceliumCall(call("mcp__mycelium__create_artifact", """{"kind":"page","title":"Board"}""", null, ToolStatus.Running)))

        assertEquals("Board", parsed.title)
        assertNull(parsed.artifactId)
    }

    @Test
    fun updateArtifactTakesTheTitleAndVersionFromTheOutput() {
        val parsed = assertIs<MyceliumCall.UpdateArtifact>(myceliumCall(call("mycelium.update_artifact", """{"artifact":"a1","note":"Tighter copy"}""", """Updated "Pricing page" to version 4.""")))

        assertEquals("a1", parsed.artifactId)
        assertEquals("Pricing page", parsed.title)
        assertEquals(4, parsed.version)
        assertEquals("Tighter copy", parsed.note)
    }

    @Test
    fun designSetArtboardKeepsTheArtboardAndTheSavedVersion() {
        val parsed = assertIs<MyceliumCall.DesignSetArtboard>(myceliumCall(call("mycelium.design_set_artboard", """{"artifact":"a1","artboard":"b2","name":"Mobile"}""", "Saved version 5.")))

        assertEquals("b2", parsed.artboard)
        assertEquals("Mobile", parsed.name)
        assertEquals(5, parsed.version)
    }

    @Test
    fun readArtifactNeedsAnArtifactOnceTheInputIsKnown() {
        assertNull(myceliumCall(call("mycelium.read_artifact", """{"artboard":"b2"}""")))
        assertEquals(MyceliumCall.ReadArtifact("a1", "b2"), myceliumCall(call("mycelium.read_artifact", """{"artifact":"a1","artboard":"b2"}""", "<!doctype html>")))
    }
}

private fun artifactWith(kind: String, thumbnailVersion: Int? = null, mediaType: String? = null) =
    MyceliumJson.decodeFromString(
        Artifact.serializer(),
        artifactJson("a1", kind).replace("\"latest_version\"", "\"thumbnail_version\":${thumbnailVersion ?: "null"},\"media_type\":${mediaType?.let { "\"$it\"" } ?: "null"},\"latest_version\""),
    )

class ArtifactPreviewTest {
    @Test
    fun aPageOrDesignWithASnapshotPreviewsAsTheSnapshot() {
        assertEquals(ArtifactPreview.Snapshot, artifactPreview(artifactWith("page", 1)))
        assertEquals(ArtifactPreview.Snapshot, artifactPreview(artifactWith("design", 2)))
    }

    @Test
    fun aPageWithoutASnapshotPreviewsAsItsIcon() {
        assertEquals(ArtifactPreview.Icon, artifactPreview(artifactWith("page")))
    }

    @Test
    fun markdownPreviewsAsText() {
        assertEquals(ArtifactPreview.Text, artifactPreview(artifactWith("markdown")))
    }

    @Test
    fun anImageFilePreviewsAsAPicture() {
        assertEquals(ArtifactPreview.Picture, artifactPreview(artifactWith("file", mediaType = "image/png")))
    }

    @Test
    fun aFileThatIsNotAnImagePreviewsAsItsIcon() {
        assertEquals(ArtifactPreview.Icon, artifactPreview(artifactWith("file", mediaType = "application/pdf")))
        assertEquals(ArtifactPreview.Icon, artifactPreview(artifactWith("file")))
    }
}

class ArtifactPreviewFetchTest {
    private val paths = mutableListOf<String>()

    private fun TestScope.store(): Store {
        val engine = MockEngine { request ->
            paths += request.url.encodedPath

            respond("bytes", HttpStatusCode.OK)
        }

        return Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)
    }

    @Test
    fun aSnapshotIsFetchedOnceForTheSameVersionAndAgainForANewOne() = runTest {
        val store = store()

        store.artifactThumbnail(artifactWith("page", 1))
        store.artifactThumbnail(artifactWith("page", 1))
        store.artifactThumbnail(artifactWith("page", 2))

        assertEquals(listOf("/api/v1/artifacts/a1/thumbnail", "/api/v1/artifacts/a1/thumbnail"), paths)
    }

    @Test
    fun noSnapshotIsFetchedWithoutAThumbnailVersion() = runTest {
        val store = store()

        assertNull(store.artifactThumbnail(artifactWith("page")))
        assertEquals(emptyList(), paths)
    }

    @Test
    fun theLatestContentIsFetchedOncePerVersion() = runTest {
        val store = store()

        assertEquals("bytes", store.artifactContent(artifactWith("markdown"))?.decodeToString())
        store.artifactContent(artifactWith("markdown"))

        assertEquals(listOf("/api/v1/artifacts/a1/versions/3/content"), paths)
    }

    @Test
    fun aFailedFetchGivesNoBytes() = runTest {
        val engine = MockEngine { respond("", HttpStatusCode.NotFound) }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        assertNull(store.artifactThumbnail(artifactWith("page", 1)))
    }
}
