package mycelium.desktop

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performMouseInput
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.rightClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private class MemoryPlatform(val saved: MutableMap<String, String> = mutableMapOf()) : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
    override fun loadPreference(key: String): String? = saved[key]
    override fun savePreference(key: String, value: String) {
        saved[key] = value
    }
}

private fun conversation(id: String, title: String, group: String? = null, pinnedAt: String? = null, status: String = "done") = """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/$id","title":"$title","status":"$status",
"acp_session_id":null,"mode":null,"model":null,"group_id":${group?.let { "\"$it\"" }},"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null,"pinned_at":${pinnedAt?.let { "\"$it\"" }}}"""

private const val GroupsJson = """[{"id":"g1","name":"slop","folders":[],"created_at":"2026-10-22T10:00:00Z"},{"id":"g2","name":"work","folders":[],"created_at":"2026-10-22T10:00:00Z"}]"""

class SidebarPinsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val patches = CopyOnWriteArrayList<Pair<String, String>>()
    private val posts = CopyOnWriteArrayList<String>()

    private fun show(conversations: List<String>, platform: MemoryPlatform = MemoryPlatform()) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Patch) {
                patches += path to String(request.body.toByteArray())

                return@MockEngine respond(conversation("w1", "Fix the build", pinnedAt = "2026-10-22T12:00:00Z"), HttpStatusCode.OK, json)
            }

            if (request.method == HttpMethod.Post) {
                posts += path
            }

            val body = when {
                path.endsWith("/conversations") -> "[${conversations.joinToString(",")}]"
                path.endsWith("/groups") -> GroupsJson
                path.endsWith("/conversations/w1") -> conversations.first()
                path.endsWith("/nodes") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }
    }

    @Test
    fun theGroupFilterShowsOnlyTheChosenGroupsConversationsAndIsRemembered() {
        val platform = MemoryPlatform()
        show(listOf(conversation("w1", "Slop work", group = "g1"), conversation("w2", "Office work", group = "g2"), conversation("w3", "Loose work")), platform)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Slop work").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Group: All groups").performClick()
        rule.onNodeWithText("slop").performClick()

        rule.onNodeWithText("Slop work").assertExists()
        rule.onAllNodesWithText("Office work").assertCountEquals(0)
        rule.onAllNodesWithText("Loose work").assertCountEquals(0)

        rule.onNodeWithContentDescription("Group: slop").performClick()
        rule.onNodeWithText("No group").performClick()

        rule.onNodeWithText("Loose work").assertExists()
        rule.onAllNodesWithText("Slop work").assertCountEquals(0)
        assertEquals("none", platform.saved["conversation-group-filter"])
    }

    @Test
    fun theGroupFilterSitsAboveNewConversation() {
        show(listOf(conversation("w1", "Slop work", group = "g1")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Slop work").fetchSemanticsNodes().isNotEmpty() }

        val filter = rule.onNodeWithContentDescription("Group: All groups").getUnclippedBoundsInRoot().top
        val newConversation = rule.onNodeWithText("New conversation").getUnclippedBoundsInRoot().top
        assertTrue(filter < newConversation, "filter $filter new conversation $newConversation")
    }

    @Test
    fun aFilteredOutGroupSaysHowManyNeedInput() {
        show(listOf(conversation("w1", "Slop work", group = "g1"), conversation("w2", "Office work", group = "g2", status = "input"), conversation("w3", "Office more", group = "g2", status = "input")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Slop work").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Group: All groups").performClick()

        rule.onNodeWithText("2 needs input").assertExists()
    }

    @Test
    fun aSavedGroupFilterIsAppliedWhenTheSidebarOpens() {
        show(listOf(conversation("w1", "Slop work", group = "g1"), conversation("w2", "Office work", group = "g2")), MemoryPlatform(mutableMapOf("conversation-group-filter" to "g2")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Office work").fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithText("Slop work").assertCountEquals(0)
    }

    @Test
    fun pinnedConversationsSitInAPinnedSectionAboveTheStatusSections() {
        show(listOf(conversation("w1", "Quiet one"), conversation("w2", "Busy one", status = "working"), conversation("w3", "Pinned one", pinnedAt = "2026-10-22T11:00:00Z")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Pinned one").fetchSemanticsNodes().isNotEmpty() }

        val pinned = rule.onNodeWithText("Pinned").getUnclippedBoundsInRoot().top
        val pinnedRow = rule.onNodeWithText("Pinned one").getUnclippedBoundsInRoot().top
        val working = rule.onNodeWithText("Working").getUnclippedBoundsInRoot().top
        assertTrue(pinned < pinnedRow && pinnedRow < working, "pinned $pinned row $pinnedRow working $working")
        rule.onAllNodesWithText("Pinned one").assertCountEquals(1)
    }

    @Test
    fun pinsAreListedInTheOrderTheyWerePinned() {
        show(listOf(conversation("w1", "Pinned later", pinnedAt = "2026-10-22T12:00:00Z"), conversation("w2", "Pinned first", pinnedAt = "2026-10-22T11:00:00Z")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Pinned first").fetchSemanticsNodes().isNotEmpty() }

        assertTrue(rule.onNodeWithText("Pinned first").getUnclippedBoundsInRoot().top < rule.onNodeWithText("Pinned later").getUnclippedBoundsInRoot().top)
    }

    @Test
    fun theRowMenuPinsAConversationAndItMovesToThePinnedSection() {
        show(listOf(conversation("w1", "Fix the build")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix the build").fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithText("Pinned").assertCountEquals(0)
        rule.onNodeWithText("Fix the build").performMouseInput { rightClick() }
        rule.onNodeWithText("Pin").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Pinned").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(listOf("/api/v1/conversations/w1" to """{"pinned":true}"""), patches)
    }

    @Test
    fun theRowMenuOffersUnpinForAPinnedConversation() {
        show(listOf(conversation("w1", "Fix the build", pinnedAt = "2026-10-22T11:00:00Z")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix the build").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Fix the build").performMouseInput { rightClick() }
        rule.onNodeWithText("Unpin").performClick()

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertEquals("""{"pinned":false}""", patches.single().second)
    }

    @Test
    fun theRowMenuRenamesAConversation() {
        show(listOf(conversation("w1", "Fix the build")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix the build").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Fix the build").performMouseInput { rightClick() }
        rule.onNodeWithText("Rename").performClick()
        rule.onNodeWithContentDescription("Conversation name").performTextReplacement("Ship the fix")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertTrue("Ship the fix" in patches.single().second, patches.single().second)
    }

    @Test
    fun theRowMenuClosesTheSessionOfAWorkingConversation() {
        show(listOf(conversation("w1", "Fix the build", status = "working")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix the build").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Fix the build").performMouseInput { rightClick() }
        rule.onNodeWithText("Close session").performClick()

        rule.waitUntil(5_000) { "/api/v1/conversations/w1/close" in posts }
        assertTrue(patches.isEmpty(), "closing a session must not archive the conversation")
    }

    @Test
    fun theRowMenuArchivesAConversation() {
        show(listOf(conversation("w1", "Fix the build")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix the build").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Fix the build").performMouseInput { rightClick() }
        rule.onNodeWithText("Archive").performClick()

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertEquals("/api/v1/conversations/w1" to """{"archived":true}""", patches.single())
    }

    @Test
    fun theHeaderMenuPinsTheOpenConversation() {
        show(listOf(conversation("w1", "Fix the build")))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix the build").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Fix the build").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Fix the build").fetchSemanticsNodes().size == 2 }
        rule.onNodeWithContentDescription("More").performClick()
        rule.onNodeWithText("Pin").performClick()

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertEquals("""{"pinned":true}""", patches.single().second)
    }
}
