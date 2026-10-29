package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.GroupsSettings
import mycelium.ui.MyceliumTheme
import mycelium.ui.SettingsScreen
import mycelium.ui.SettingsSection
import mycelium.ui.verticalScrollWithBar
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val GroupNodes = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]}]"""

private const val SlopGroup = """{"id":"g1","name":"slop","folders":[{"id":"f1","node_id":null,"prefix":"/home/claude/projects"},{"id":"f2","node_id":"n1","prefix":"/srv/slop"}],"created_at":"2026-10-22T10:00:00Z"}"""

private const val WorkGroup = """{"id":"g2","name":"work","folders":[],"created_at":"2026-10-22T10:00:00Z"}"""

private const val HomelabGroup = """{"id":"g3","name":"homelab","folders":[],"created_at":"2026-10-22T10:00:00Z"}"""

private const val GroupProjects = """[{"id":"p1","node_id":"n1","name":"mycelium","path":"/p/mycelium","group_id":"g1","created_at":"2026-10-22T10:00:00Z"}]"""

private const val GroupLayer = """{"layer":"g1","name":"slop","path":null,"text":"Use British English.","version":null}"""

private const val GroupProposal = """[{"id":"p1","conversation_id":null,"conversation_title":null,"layer":"g1","layer_name":"slop","base_revision":"abc","base_text":"a","text":"b","reason":"You said: use British English.","state":"proposed","note":null,"out_of_date":false,"created_at":"2026-10-23T10:00:00Z"}]"""

private const val InstructionsDocument = """{"source":null,"revision":"abc","layers":[{"layer":"global","name":"Global","path":null,"text":"## Diffs","version":null}]}"""


class GroupsSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<Triple<HttpMethod, String, String>>())

    private fun requestTo(method: HttpMethod, pathEnd: String): Triple<HttpMethod, String, String> {
        rule.waitUntil(timeoutMillis = 5_000) { requests.toList().any { it.first == method && it.second.endsWith(pathEnd) } }

        return requests.toList().first { it.first == method && it.second.endsWith(pathEnd) }
    }

    private fun store(groups: String, experiments: String = EnabledExperiments, proposals: String = "[]"): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        var created = false
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += Triple(request.method, path, request.body.toByteArray().decodeToString())
            created = created || (request.method == HttpMethod.Post && path.endsWith("/groups"))
            val body = when {
                path.endsWith("/experiments") -> experiments
                path.endsWith("/conversations") -> "[]"
                path.endsWith("/nodes") -> GroupNodes
                path.endsWith("/groups/g1/instructions") -> GroupLayer
                path.endsWith("/instructions") -> InstructionsDocument
                path.endsWith("/extensions") -> """{"generated_ui":true,"rich_markdown":true,"mycelium_tools":true}"""
                path.endsWith("/forges") -> "[]"
                path.endsWith("/groups") && request.method == HttpMethod.Post -> HomelabGroup
                path.endsWith("/groups") -> listOfNotNull(groups.ifBlank { null }, HomelabGroup.takeIf { created }).joinToString(",", "[", "]")
                path.endsWith("/projects") -> GroupProjects
                path.endsWith("/instruction-proposals") -> proposals
                path.contains("/groups/") && request.method == HttpMethod.Patch -> """{"id":"g1","name":"homelab","folders":[],"created_at":"2026-10-22T10:00:00Z"}"""
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        return store
    }

    private fun show(groups: String = "$SlopGroup,$WorkGroup") {
        val store = store(groups)
        rule.setContent { MyceliumTheme(true) { var open by remember { mutableStateOf<String?>(null) }; Box(Modifier.verticalScrollWithBar()) { GroupsSettings(store, open, { open = it }) } } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Groups").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun open(name: String) {
        rule.onNodeWithContentDescription("Open the $name group").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Group sections").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun tab(name: String) {
        rule.onNodeWithText(name).performClick()
    }

    private fun settings(experiments: String) {
        val store = store(SlopGroup, experiments)
        rule.setContent { MyceliumTheme(true) { Box(Modifier.height(900.dp)) { SettingsScreen(store, Me("u", "sam@example.com", "Sam"), "https://mycelium.test", onSignOut = {}) } } }
        rule.waitForIdle()
    }

    @Test
    fun settingsListsGroupsUnderKnowledgeWhenTheExperimentIsOn() {
        settings(EnabledExperiments)

        rule.onNodeWithText("Groups").assertExists()
    }

    @Test
    fun anEmptyListExplainsGroupsAndOffersNewGroup() {
        show("")

        rule.onNodeWithText("No groups", substring = true).assertExists()
        rule.onNodeWithText("New group").assertExists()
    }

    @Test
    fun aNewGroupIsCreatedFromItsNameAndOpens() {
        show()

        rule.onNodeWithText("New group").performClick()
        rule.onNodeWithContentDescription("Group name").performTextInput("homelab")
        rule.onNodeWithText("Save").performClick()

        assertEquals("""{"name":"homelab","icon":"group"}""", requestTo(HttpMethod.Post, "/groups").third)
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Group sections").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aGroupIsRenamedFromItsRowMenu() {
        show()

        rule.onNodeWithContentDescription("More for slop group").performClick()
        rule.onNodeWithText("Rename").performClick()
        rule.onNodeWithContentDescription("Group name").performTextClearance()
        rule.onNodeWithContentDescription("Group name").performTextInput("homelab")
        rule.onNodeWithText("Save").performClick()

        assertEquals("""{"name":"homelab","icon":"group"}""", requestTo(HttpMethod.Patch, "/groups/g1").third)
    }

    @Test
    fun aNewGroupCanBeGivenOneOfTheFixedIcons() {
        show()

        rule.onNodeWithText("New group").performClick()
        rule.onNodeWithContentDescription("Group name").performTextInput("homelab")
        rule.onNodeWithContentDescription("Briefcase icon").performClick()
        rule.onNodeWithText("Save").performClick()

        assertEquals("""{"name":"homelab","icon":"briefcase"}""", requestTo(HttpMethod.Post, "/groups").third)
    }

    @Test
    fun deletingAGroupAsksFirstThenSendsTheDelete() {
        show()

        rule.onNodeWithContentDescription("More for work group").performClick()
        rule.onNodeWithText("Delete").performClick()

        rule.onNodeWithText("Delete the work group?", substring = true).assertExists()
        assertTrue(requests.none { it.first == HttpMethod.Delete })

        rule.onNodeWithText("Delete work").performClick()

        requestTo(HttpMethod.Delete, "/groups/g2")
    }

    @Test
    fun aFolderIsAddedToTheOpenGroupForAnyNode() {
        show()
        open("slop")
        tab("Folders")

        rule.onNodeWithText("Add folder").performScrollTo().performClick()
        rule.onNodeWithContentDescription("Folder").performTextInput("/home/claude/other")
        rule.onNodeWithText("Save").performClick()

        assertEquals("""{"prefix":"/home/claude/other"}""", requestTo(HttpMethod.Post, "/groups/g1/folders").third)
    }

    @Test
    fun aFolderIsRemovedByItsName() {
        show()
        open("slop")
        tab("Folders")

        rule.onNodeWithContentDescription("More for folder /srv/slop in slop").performScrollTo().performClick()
        rule.onNodeWithText("Remove folder").performClick()

        requestTo(HttpMethod.Delete, "/groups/g1/folders/f2")
    }

    @Test
    fun savingGroupInstructionsSendsTheTextToTheGroupRoute() {
        show()
        open("slop")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Use British English.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Stored in Mycelium").assertExists()
        rule.onNodeWithText("Save").performScrollTo().performClick()

        assertEquals("""{"text":"Use British English."}""", requestTo(HttpMethod.Put, "/groups/g1/instructions").third)
    }

    @Test
    fun theGroupPageListsItsProjectsWithoutAMenu() {
        show()
        open("slop")
        tab("Projects")

        rule.onNodeWithText("mycelium").assertExists()
        assertEquals(0, rule.onAllNodesWithText("Remove from slop").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("More for project mycelium in slop").fetchSemanticsNodes().size)
    }

    @Test
    fun aProposalForAGroupLayerOpensThatGroup() {
        val store = store(SlopGroup, proposals = GroupProposal)
        rule.setContent { MyceliumTheme(true) { Box(Modifier.height(900.dp)) { SettingsScreen(store, Me("u", "sam@example.com", "Sam"), "https://mycelium.test", startOn = SettingsSection.Instructions, onSignOut = {}) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open the slop group").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Open the slop group").performScrollTo().performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Group sections").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theGroupPageShowsItsPendingProposalAboveItsInstructionsAndNoOtherLayers() {
        val other = GroupProposal.replace("\"id\":\"p1\"", "\"id\":\"p2\"").replace("\"layer\":\"g1\",\"layer_name\":\"slop\"", "\"layer\":\"global\",\"layer_name\":\"Global\"")
        val store = store(SlopGroup, proposals = GroupProposal.trimEnd(']') + "," + other.trimStart('['))
        rule.setContent { MyceliumTheme(true) { Box(Modifier.height(900.dp)) { SettingsScreen(store, Me("u", "sam@example.com", "Sam"), "https://mycelium.test", startOn = SettingsSection.Groups, startGroup = "g1", onSignOut = {}) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Proposed edit to the slop instructions").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("Proposed edit to the Global instructions").fetchSemanticsNodes().size)
    }
}
