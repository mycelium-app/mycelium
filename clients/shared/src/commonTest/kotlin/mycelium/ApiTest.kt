package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.MockRequestHandleScope
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.client.request.HttpRequestData
import io.ktor.client.request.HttpResponseData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.ApiException
import mycelium.api.ConnectForgeRequest
import mycelium.api.CreateConversationRequest
import mycelium.api.CreateProjectRequest
import mycelium.api.ConversationStatus
import mycelium.api.DeviceKind
import mycelium.api.DevicePoll
import mycelium.api.DeviceRequest
import mycelium.api.MyceliumApi
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertIs

private val JsonHeaders = headersOf(HttpHeaders.ContentType, "application/json")

private fun MockRequestHandleScope.ok(body: String): HttpResponseData = respond(body, HttpStatusCode.OK, JsonHeaders)

private fun apiWith(token: String? = null, handler: suspend MockRequestHandleScope.(HttpRequestData) -> HttpResponseData): Pair<MyceliumApi, MutableList<HttpRequestData>> {
    val seen = mutableListOf<HttpRequestData>()
    val engine = MockEngine { request ->
        seen += request
        handler(request)
    }

    return MyceliumApi("https://mycelium.test", token, engine) to seen
}

private const val ConversationJson = """{"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/work/midden","title":"Bump","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val MemoryJson = """{"id":"m1","text":"Staging listens on 5433","scopes":["global"],"created_by":"laptop/Fix login","conversation_id":null,
"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val ForgeJson = """{"id":"f1","kind":"github","base_url":"https://github.com","account":"samexample","token_hint":"wxyz",
"organisations":["samexample","acme-org"],"created_at":"2026-10-22T10:00:00Z"}"""

private const val ProjectJson = """{"id":"p1","node_id":"n1","name":"acme-backups","path":"/home/sam/projects/acme-org/acme-backups","forge_id":"f1",
"repository":"acme-org/acme-backups","created_at":"2026-10-22T10:00:00Z"}"""

class ApiTest {
    @Test
    fun startingATerminalPostsItsSizeAndReturnsTheId() = runTest {
        val (api, seen) = apiWith { ok("""{"terminal_id":"t1"}""") }

        val started = api.startTerminal("c1", 80, 24)

        assertEquals("t1", started.terminalId)
        assertEquals("/api/v1/conversations/c1/terminal", seen.single().url.encodedPath)
        assertEquals("""{"cols":80,"rows":24}""", String(seen.single().body.toByteArray()))
    }

    @Test
    fun terminalInputResizeAndStopUseTheTerminalRoutes() = runTest {
        val (api, seen) = apiWith { respond("", HttpStatusCode.NoContent) }

        api.terminalInput("t1", "bHM=")
        api.resizeTerminal("t1", 100, 30)
        api.stopTerminal("t1")

        assertEquals(
            listOf(HttpMethod.Post to "/api/v1/terminals/t1/input", HttpMethod.Post to "/api/v1/terminals/t1/resize", HttpMethod.Delete to "/api/v1/terminals/t1"),
            seen.map { it.method to it.url.encodedPath },
        )
        assertEquals("""{"data":"bHM="}""", String(seen[0].body.toByteArray()))
        assertEquals("""{"cols":100,"rows":30}""", String(seen[1].body.toByteArray()))
    }

    @Test
    fun listsConversationsAndSendsTheArchivedFlag() = runTest {
        val (api, seen) = apiWith { ok("[$ConversationJson]") }

        val rows = api.conversations(archived = false)

        assertEquals(ConversationStatus.Working, rows.single().status)
        assertEquals("https://mycelium.test/api/v1/conversations?archived=false", seen.single().url.toString())
    }

    @Test
    fun clientTokenIsSentAsABearerWithTheClientPrefix() = runTest {
        val (api, seen) = apiWith("abc123") { ok("[]") }

        api.nodes()

        assertEquals("Bearer myc_client_abc123", seen.single().headers[HttpHeaders.Authorization])
    }

    @Test
    fun noTokenSendsNoAuthorizationHeaderSoTheBrowserCookieApplies() = runTest {
        val (api, seen) = apiWith { ok("[]") }

        api.nodes()

        assertEquals(null, seen.single().headers[HttpHeaders.Authorization])
    }

    @Test
    fun theControlPlaneVersionIsReadFromItsVersionRoute() = runTest {
        val (api, _) = apiWith { request ->
            respond(if (request.url.encodedPath == "/api/v1/version") """{"version":"v0.3.6"}""" else "{}", HttpStatusCode.OK, JsonHeaders)
        }

        assertEquals("v0.3.6", api.version())
    }

    @Test
    fun anOlderControlPlaneWithoutTheRouteAsksForAnUpdateRatherThanACheckOfTheAddress() = runTest {
        val (api, _) = apiWith {
            respond("""{"error":"not_found","message":"That address was not found. Check the address and try again."}""", HttpStatusCode.NotFound, JsonHeaders)
        }

        val failure = assertFailsWith<ApiException> { api.nodes() }

        assertEquals("unknown_route", failure.code)
        assertEquals("This control plane does not offer that yet. Update it to the latest version of Mycelium, then try again.", failure.message)
    }

    @Test
    fun errorBodyBecomesAnApiExceptionWithTheServersSentence() = runTest {
        val (api, _) = apiWith {
            respond(
                """{"error":"not_found","message":"That conversation does not exist. Check the link and try again."}""",
                HttpStatusCode.NotFound,
                JsonHeaders,
            )
        }

        val failure = assertFailsWith<ApiException> { api.conversation("missing") }

        assertEquals(404, failure.status)
        assertEquals("not_found", failure.code)
        assertEquals("That conversation does not exist. Check the link and try again.", failure.message)
    }

    @Test
    fun anUnparseableErrorStillGivesAnActionableMessage() = runTest {
        val (api, _) = apiWith { respond("<html>bad gateway</html>", HttpStatusCode.BadGateway) }

        val failure = assertFailsWith<ApiException> { api.nodes() }

        assertEquals(502, failure.status)
        assertEquals("The server answered with status 502. Try again in a moment.", failure.message)
    }

    @Test
    fun promptPostsTheTextToTheConversation() = runTest {
        val (api, seen) = apiWith { respond("", HttpStatusCode.NoContent) }

        api.sendPrompt("c1", "Also run the linter")

        val request = seen.single()
        assertEquals(HttpMethod.Post, request.method)
        assertEquals("/api/v1/conversations/c1/prompt", request.url.encodedPath)
        assertEquals("""{"text":"Also run the linter"}""", String(request.body.toByteArray()))
    }

    @Test
    fun permissionAnswerUsesTheWireFieldNames() = runTest {
        val (api, seen) = apiWith { respond("", HttpStatusCode.NoContent) }

        api.answerPermission("c1", "r1", "allow")

        assertEquals("""{"request_id":"r1","option_id":"allow"}""", String(seen.single().body.toByteArray()))
    }

    @Test
    fun sendNowAndRemoveQueuedHitTheQueueRoutes() = runTest {
        val (api, seen) = apiWith { respond("", HttpStatusCode.NoContent) }

        api.sendQueuedNow("c1", "q1")
        api.removeQueued("c1", "q1")

        assertEquals(
            listOf(HttpMethod.Post to "/api/v1/conversations/c1/queue/q1/send-now", HttpMethod.Delete to "/api/v1/conversations/c1/queue/q1"),
            seen.map { it.method to it.url.encodedPath },
        )
    }

    @Test
    fun devicePollIsPendingUntilTheCodeIsApproved() = runTest {
        var approved = false
        val (api, _) = apiWith {
            if (approved) {
                ok("""{"kind":"client","id":"d1","token":"secret"}""")
            } else {
                respond("""{"error":"authorization_pending","message":"Waiting."}""", HttpStatusCode.BadRequest, JsonHeaders)
            }
        }

        assertEquals(DevicePoll.Pending, api.pollDevice("dc"))

        approved = true
        val result = assertIs<DevicePoll.Approved>(api.pollDevice("dc"))
        assertEquals("secret", result.token.token)
    }

    @Test
    fun devicePollReportsAnExpiredCodeAsRejected() = runTest {
        val (api, _) = apiWith {
            respond("""{"error":"expired_token","message":"This code has expired. Start again."}""", HttpStatusCode.BadRequest, JsonHeaders)
        }

        val result = assertIs<DevicePoll.Rejected>(api.pollDevice("dc"))

        assertEquals("expired_token", result.code)
    }

    @Test
    fun startingADeviceCodeSendsTheClientKind() = runTest {
        val (api, seen) = apiWith {
            ok("""{"device_code":"dc","user_code":"WDJB-MJHT","verification_uri":"https://x/approve","verification_uri_complete":"https://x/approve?code=WDJB-MJHT","interval":5,"expires_in":600}""")
        }

        val response = api.startDevice(DeviceRequest(DeviceKind.Client, "Desktop", "host", "linux", "x86_64"))

        assertEquals("WDJB-MJHT", response.userCode)
        assertEquals(5L, response.interval)
        assertEquals(
            """{"kind":"client","label":"Desktop","hostname":"host","os":"linux","arch":"x86_64"}""",
            String(seen.single().body.toByteArray()),
        )
    }

    @Test
    fun memoriesDecodeAndASearchSendsTheTrimmedQuery() = runTest {
        val (api, seen) = apiWith { ok("[$MemoryJson]") }

        val memory = api.memories(" staging ").single()

        assertEquals("Staging listens on 5433", memory.text)
        assertEquals(listOf("global"), memory.scopes)
        assertEquals("laptop/Fix login", memory.createdBy)
        assertEquals("https://mycelium.test/api/v1/memories?query=staging", seen.single().url.toString())
    }

    @Test
    fun listingMemoriesWithoutAQuerySendsNone() = runTest {
        val (api, seen) = apiWith { ok("[]") }

        api.memories("  ")

        assertEquals("https://mycelium.test/api/v1/memories", seen.single().url.toString())
    }

    @Test
    fun editingAMemoryPatchesItsTextAndReturnsTheSavedMemory() = runTest {
        val (api, seen) = apiWith { ok(MemoryJson) }

        val saved = api.updateMemory("m1", "Staging listens on 5433")

        assertEquals(HttpMethod.Patch, seen.single().method)
        assertEquals("https://mycelium.test/api/v1/memories/m1", seen.single().url.toString())
        assertEquals("""{"text":"Staging listens on 5433"}""", String(seen.single().body.toByteArray()))
        assertEquals("m1", saved.id)
    }

    @Test
    fun editingAMemoryToBlankTextFailsWithTheServersSentence() = runTest {
        val (api, _) = apiWith {
            respond("""{"error":"text_required","message":"Enter what to remember."}""", HttpStatusCode.BadRequest, JsonHeaders)
        }

        val failure = assertFailsWith<ApiException> { api.updateMemory("m1", "") }

        assertEquals("Enter what to remember.", failure.message)
    }

    @Test
    fun deletingAMemoryUsesTheDeleteVerb() = runTest {
        val (api, seen) = apiWith { respond("", HttpStatusCode.NoContent) }

        api.deleteMemory("m1")

        assertEquals(HttpMethod.Delete, seen.single().method)
        assertEquals("https://mycelium.test/api/v1/memories/m1", seen.single().url.toString())
    }

    @Test
    fun forgesDecodeTheAccountAndOrganisationsAndNeverHoldAToken() = runTest {
        val leaking = ForgeJson.replace("\"token_hint\"", "\"token\":\"ghp_secret\",\"token_hint\"")
        val (api, seen) = apiWith { ok("[$leaking]") }

        val forge = api.forges().single()

        assertEquals("samexample", forge.account)
        assertEquals(listOf("samexample", "acme-org"), forge.organisations)
        assertEquals("wxyz", forge.tokenHint)
        assertEquals("https://mycelium.test/api/v1/forges", seen.single().url.toString())
        assertFalse(forge.toString().contains("ghp_secret"))
    }

    @Test
    fun connectingAForgePostsTheKindBaseUrlAndTokenAndReturnsTheForge() = runTest {
        val (api, seen) = apiWith { respond(ForgeJson, HttpStatusCode.Created, JsonHeaders) }

        val forge = api.connectForge(ConnectForgeRequest("gitlab", "https://gitlab.example.com", "glpat-1"))

        assertEquals(HttpMethod.Post, seen.single().method)
        assertEquals("""{"kind":"gitlab","base_url":"https://gitlab.example.com","token":"glpat-1"}""", String(seen.single().body.toByteArray()))
        assertEquals("f1", forge.id)
    }

    @Test
    fun connectingWithoutABaseUrlLeavesItOutSoTheServerPicksTheDefault() = runTest {
        val (api, seen) = apiWith { respond(ForgeJson, HttpStatusCode.Created, JsonHeaders) }

        api.connectForge(ConnectForgeRequest("github", null, "ghp_1"))

        assertEquals("""{"kind":"github","token":"ghp_1"}""", String(seen.single().body.toByteArray()))
    }

    @Test
    fun aRejectedForgeTokenFailsWithTheServersSentence() = runTest {
        val (api, _) = apiWith {
            respond("""{"error":"forge_token_rejected","message":"GitHub did not accept that token. Check it and try again."}""", HttpStatusCode.BadRequest, JsonHeaders)
        }

        val failure = assertFailsWith<ApiException> { api.connectForge(ConnectForgeRequest("github", null, "bad")) }

        assertEquals("forge_token_rejected", failure.code)
        assertEquals("GitHub did not accept that token. Check it and try again.", failure.message)
    }

    @Test
    fun disconnectingAForgeUsesTheDeleteVerb() = runTest {
        val (api, seen) = apiWith { respond("", HttpStatusCode.NoContent) }

        api.disconnectForge("f1")

        assertEquals(HttpMethod.Delete, seen.single().method)
        assertEquals("https://mycelium.test/api/v1/forges/f1", seen.single().url.toString())
    }

    @Test
    fun repositoriesDecodeAndFiltersAreSentOnlyWhenSet() = runTest {
        val (api, seen) = apiWith {
            ok(
                """{"repositories":[{"forge_id":"f1","owner":"acme-org","name":"acme-backups","full_name":"acme-org/acme-backups","description":null,"private":true,"default_branch":"main"}],
                "organisations":[{"forge_id":"f1","name":"acme-org"}],"failures":[{"forge_id":"f2","message":"GitLab could not be reached."}]}""",
            )
        }

        val listing = api.repositories(forge = "f1", organisation = "acme-org", query = " backups ")

        assertEquals("acme-org/acme-backups", listing.repositories.single().fullName)
        assertEquals(true, listing.repositories.single().private)
        assertEquals("acme-org", listing.organisations.single().name)
        assertEquals("GitLab could not be reached.", listing.failures.single().message)
        assertEquals("https://mycelium.test/api/v1/repositories?forge=f1&organisation=acme-org&q=backups", seen.single().url.toString())
    }

    @Test
    fun listingRepositoriesWithoutFiltersSendsNone() = runTest {
        val (api, seen) = apiWith { ok("""{"repositories":[],"organisations":[],"failures":[]}""") }

        api.repositories()

        assertEquals("https://mycelium.test/api/v1/repositories", seen.single().url.toString())
    }

    @Test
    fun projectsDecodeWithAndWithoutAForge() = runTest {
        val folder = """{"id":"p2","node_id":"n1","name":"app","path":"/w/app","forge_id":null,"repository":null,"created_at":"2026-10-22T10:00:00Z"}"""
        val (api, seen) = apiWith { ok("[$ProjectJson,$folder]") }

        val projects = api.projects()

        assertEquals("acme-org/acme-backups", projects[0].repository)
        assertEquals("f1", projects[0].forgeId)
        assertEquals(null, projects[1].forgeId)
        assertEquals("https://mycelium.test/api/v1/projects", seen.single().url.toString())
    }

    @Test
    fun cloningARepositoryPostsTheNodeForgeAndRepository() = runTest {
        val (api, seen) = apiWith { respond(ProjectJson, HttpStatusCode.Created, JsonHeaders) }

        val project = api.createProject(CreateProjectRequest("n1", "f1", "acme-org/acme-backups"))

        assertEquals("""{"node_id":"n1","forge_id":"f1","repository":"acme-org/acme-backups"}""", String(seen.single().body.toByteArray()))
        assertEquals("p1", project.id)
    }

    @Test
    fun registeringAFolderPostsTheNodeAndPathOnly() = runTest {
        val (api, seen) = apiWith { respond(ProjectJson, HttpStatusCode.Created, JsonHeaders) }

        api.createProject(CreateProjectRequest("n1", path = "work/app"))

        assertEquals("""{"node_id":"n1","path":"work/app"}""", String(seen.single().body.toByteArray()))
    }

    @Test
    fun aRefusedCloneFailsWithGitsMessage() = runTest {
        val (api, _) = apiWith { respond("""{"error":"git_failed","message":"repository not found"}""", HttpStatusCode.Conflict, JsonHeaders) }

        val failure = assertFailsWith<ApiException> { api.createProject(CreateProjectRequest("n1", "f1", "acme-org/missing")) }

        assertEquals("git_failed", failure.code)
        assertEquals("repository not found", failure.message)
    }

    @Test
    fun aConversationFromAProjectSendsProjectIdAndNoFolder() = runTest {
        val (api, seen) = apiWith { ok(ConversationJson) }

        api.createConversation(CreateConversationRequest("n1", "claude", projectId = "p1"))

        assertEquals("""{"node_id":"n1","agent":"claude","project_id":"p1"}""", String(seen.single().body.toByteArray()))
    }

    @Test
    fun aConversationFromAFolderStillSendsTheFolder() = runTest {
        val (api, seen) = apiWith { ok(ConversationJson) }

        api.createConversation(CreateConversationRequest("n1", "claude", cwd = "/work/midden"))

        assertEquals("""{"node_id":"n1","agent":"claude","cwd":"/work/midden"}""", String(seen.single().body.toByteArray()))
    }

    @Test
    fun nodesDecodeAgentsAndLogins() = runTest {
        val (api, _) = apiWith {
            ok(
                """[{"id":"n1","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
                "last_seen_at":null,"agents":[{"agent":"codex","version":"0.163.0","available":true}],
                "logins":[{"agent":"codex","method":"subscription","state":"expired"}]}]""",
            )
        }

        val node = api.nodes().single()

        assertEquals("codex", node.agents.single().agent)
        assertEquals(mycelium.api.LoginState.Expired, node.logins.single().state)
    }

    @Test
    fun importStatusesDecodeAndTheImportPostsTheNodeAndAgent() = runTest {
        val (api, seen) = apiWith { request ->
            if (request.method == HttpMethod.Post) ok("""{"imported":3,"skipped":1}""") else ok("""[{"node_id":"n1","agent":"claude","found":5,"imported":2}]""")
        }

        val status = api.imports().single()
        val result = api.startImport(mycelium.api.ImportRequest("n1", "claude"))

        assertEquals(3, mycelium.ui.pendingImports(status))
        assertEquals(3, result.imported)
        assertEquals(1, result.skipped)
        assertEquals("""{"node_id":"n1","agent":"claude"}""", String(seen.last().body.toByteArray()))
    }

    @Test
    fun anAgentWithNothingLeftToImportHasNoPendingImports() {
        assertEquals(0, mycelium.ui.pendingImports(mycelium.api.ImportStatus("n1", "codex", found = 4, imported = 4)))
        assertEquals(0, mycelium.ui.pendingImports(null))
    }

    @Test
    fun indexingDecodesTheCountsPerNode() = runTest {
        val (api, seen) = apiWith { ok("""{"conversations":7,"entries":40,"nodes":[{"node_id":"n1","transcripts":9,"imported":6,"conversations":7}]}""") }

        val indexing = api.indexing()

        assertEquals(40, indexing.entries)
        assertEquals(6, indexing.nodes.single().imported)
        assertEquals("https://mycelium.test/api/v1/indexing", seen.single().url.toString())
    }

    @Test
    fun searchSendsTheQueryAndDecodesTheSnippet() = runTest {
        val (api, seen) = apiWith {
            ok("""[{"conversation_id":"c1","title":"Rotate keys","node_id":"n1","updated_at":"2026-10-01T10:00:00Z","snippet":"the **keystore** is rotated"}]""")
        }

        val hit = api.search("key store").single()

        assertEquals("the **keystore** is rotated", hit.snippet)
        assertEquals("c1", hit.conversationId)
        assertEquals("https://mycelium.test/api/v1/search?q=key+store&limit=20", seen.single().url.toString())
    }

    @Test
    fun anEmptySearchSurfacesTheServersMessage() = runTest {
        val (api, _) = apiWith { respond("""{"error":"query_required","message":"Enter something to search for."}""", HttpStatusCode.BadRequest, JsonHeaders) }

        val failure = assertFailsWith<ApiException> { api.search("") }

        assertEquals("Enter something to search for.", failure.message)
    }

    @Test
    fun gitStatusLogAndDiffAreReadFromTheGitRoutes() = runTest {
        val (api, seen) = apiWith { request ->
            when (request.url.encodedPath) {
                "/api/v1/conversations/c1/git/status" -> ok("""{"branch":"main","upstream":null,"ahead":1,"behind":0,"files":[{"path":"a.kt","staged":true,"unstaged":false,"untracked":false,"additions":3,"deletions":1}]}""")
                "/api/v1/conversations/c1/git/log" -> ok("""[{"hash":"abc1234","author":"Ada","date":"2026-10-22T10:00:00+00:00","subject":"Fix"}]""")
                else -> ok("""{"diff":"@@","truncated":true}""")
            }
        }

        val status = api.gitStatus("c1")
        val log = api.gitLog("c1", 10)
        val diff = api.gitDiff("c1", "src/a.kt", staged = true)

        assertEquals(1, status.ahead)
        assertEquals(null, status.upstream)
        assertEquals(3, status.files.single().additions)
        assertEquals("Fix", log.single().subject)
        assertEquals(true, diff.truncated)
        assertEquals("limit=10", seen[1].url.encodedQuery)
        assertEquals("path=src%2Fa.kt&staged=true", seen[2].url.encodedQuery)
    }

    @Test
    fun gitActionsPostToTheirOwnRoutesWithTheirBodies() = runTest {
        val (api, seen) = apiWith { ok("{}") }

        api.gitStage("c1", listOf("a", "b"))
        api.gitUnstage("c1", listOf("a"))
        api.gitStageAll("c1")
        api.gitCommit("c1", "Fix it")
        api.gitFetch("c1")
        api.gitUndo("c1")

        assertEquals(
            listOf("stage", "unstage", "stage-all", "commit", "fetch", "undo").map { HttpMethod.Post to "/api/v1/conversations/c1/git/$it" },
            seen.map { it.method to it.url.encodedPath },
        )
        assertEquals("""{"paths":["a","b"]}""", String(seen[0].body.toByteArray()))
        assertEquals("""{"message":"Fix it"}""", String(seen[3].body.toByteArray()))
    }

    @Test
    fun aFailedGitCommandSurfacesGitsOwnMessage() = runTest {
        val (api, _) = apiWith { respond("""{"error":"git_failed","message":"fatal: no remote repository specified"}""", HttpStatusCode.Conflict, JsonHeaders) }

        val failure = assertFailsWith<ApiException> { api.gitFetch("c1") }

        assertEquals("git_failed", failure.code)
        assertEquals("fatal: no remote repository specified", failure.message)
    }
}
