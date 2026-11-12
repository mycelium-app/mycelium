package mycelium

import kotlinx.serialization.decodeFromString
import mycelium.api.MyceliumJson
import mycelium.api.Conversation
import mycelium.api.NodeSummary
import mycelium.api.Project
import mycelium.ui.runOnNode
import kotlin.test.Test
import kotlin.test.assertEquals

private fun node(id: String, name: String) = MyceliumJson.decodeFromString<NodeSummary>("""{"id":"$id","name":"$name","hostname":"$name","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false}""")

private fun conversationOn(node: String, updatedAt: String) = MyceliumJson.decodeFromString<Conversation>(
    """{"id":"c-$node","owner":"u","node_id":"$node","agent":"claude","cwd":"/p","title":"t","status":"done","project_id":"p1","created_at":"2026-10-22T10:00:00Z","updated_at":"$updatedAt"}""",
)

private const val TwoNodes = """{"id":"p1","node_id":"atlas","name":"backups","path":"/a","repository":"acme-org/backups","forge_id":"f1",
"checkouts":[{"id":"k1","node_id":"atlas","path":"/a","created_at":"2026-10-22T10:00:00Z"},{"id":"k2","node_id":"orion","path":"/o","created_at":"2026-10-23T10:00:00Z"}],"test_command":"make test","created_at":"2026-10-22T10:00:00Z"}"""

class ProjectCheckoutsTest {
    private val atlas = node("atlas", "atlas")
    private val orion = node("orion", "orion")
    private val willow = node("willow", "willow")

    @Test
    fun aProjectFromAnOlderServerHasOneCheckoutBuiltFromItsNodeAndFolder() {
        val project = MyceliumJson.decodeFromString<Project>("""{"id":"p1","node_id":"atlas","name":"backups","path":"/a","created_at":"2026-10-22T10:00:00Z"}""")

        assertEquals(listOf("atlas" to "/a"), project.places.map { it.nodeId to it.path })
    }

    @Test
    fun aProjectWithCheckoutsListsThemAndKeepsTheTestCommand() {
        val project = MyceliumJson.decodeFromString<Project>(TwoNodes)

        assertEquals(listOf("atlas", "orion"), project.places.map { it.nodeId })
        assertEquals("make test", project.testCommand)
    }

    @Test
    fun theRunOnNodeIsTheNodeOfTheLastConversationInTheProject() {
        val project = MyceliumJson.decodeFromString<Project>(TwoNodes)
        val newest = listOf(conversationOn("orion", "2026-10-23T12:00:00Z"), conversationOn("atlas", "2026-10-22T12:00:00Z"))

        assertEquals("orion", runOnNode(project, listOf(atlas, orion, willow), newest, null)?.id)
    }

    @Test
    fun aKeptNodeWithACheckoutBeatsTheLastUsedNodeAndOneWithoutIsIgnored() {
        val project = MyceliumJson.decodeFromString<Project>(TwoNodes)
        val newest = listOf(conversationOn("orion", "2026-10-23T12:00:00Z"))

        assertEquals("atlas", runOnNode(project, listOf(atlas, orion, willow), newest, "atlas")?.id)
        assertEquals("orion", runOnNode(project, listOf(atlas, orion, willow), newest, "willow")?.id)
    }

    @Test
    fun anOfflineNodeIsNeverTheRunOnNodeAndNoOnlineCheckoutMeansNone() {
        val project = MyceliumJson.decodeFromString<Project>(TwoNodes)

        assertEquals("atlas", runOnNode(project, listOf(atlas, willow), emptyList(), null)?.id)
        assertEquals(null, runOnNode(project, listOf(willow), emptyList(), null))
    }
}
