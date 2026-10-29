package mycelium

import mycelium.state.PasteDisplay
import mycelium.state.nodeIdFromConfig
import mycelium.state.serialisedPastes
import kotlin.test.Test
import kotlin.test.assertEquals

class LocalNodeTest {
    @Test
    fun theNodeIdIsReadFromTheNodeConfig() {
        val config = "control_plane = \"https://mycelium.example.com\"\nnode_id = \"3F2A9C1E-1111-4222-8333-444455556666\"\ntoken = \"x\"\n"

        assertEquals("3f2a9c1e-1111-4222-8333-444455556666", nodeIdFromConfig(config))
    }

    @Test
    fun aNodeThatHasNotJoinedYetHasNoId() {
        assertEquals(null, nodeIdFromConfig("node_id = \"00000000-0000-0000-0000-000000000000\""))
        assertEquals(null, nodeIdFromConfig("token = \"x\""))
    }

    @Test
    fun aCitationIsShownAsItsQuoteAndSentAsAQuoteBlock() {
        val marker = ''
        val citations = mapOf(marker to "the retry delay")

        assertEquals(" \"the retry delay\" ", PasteDisplay("$marker", emptyMap(), emptyMap(), citations).labelOf(marker)?.let { " $it " })
        assertEquals("see\n> the retry delay\n now", serialisedPastes("see$marker now", emptyMap(), citations))
    }
}
