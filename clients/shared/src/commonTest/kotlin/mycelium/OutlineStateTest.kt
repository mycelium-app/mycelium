package mycelium

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.ChatToolServer
import mycelium.api.Event
import mycelium.api.OutlineDocumentNode
import mycelium.state.OutlinePage
import mycelium.state.outlineAncestors
import mycelium.state.outlineIntegrations
import mycelium.state.outlinePageFrom
import mycelium.state.outlinePageOf
import mycelium.state.outlineRows
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class OutlineStateTest {
    private val rota = OutlineDocumentNode("d-rota", "Rota", "/doc/rota-r1")
    private val onCall = OutlineDocumentNode("d-oncall", "On-call", "/doc/on-call-o1", listOf(rota))
    private val engineering = OutlineDocumentNode("d-eng", "Engineering", "/doc/engineering-e1", listOf(onCall))
    private val norms = OutlineDocumentNode("d-norms", "Team norms", "/doc/team-norms-t1")

    private fun server(id: String, app: String? = "outline", groups: List<String> = emptyList(), enabled: Boolean = true) =
        ChatToolServer(id, id, url = "https://docs.example.org/mcp", settings = app?.let { JsonObject(mapOf("app" to JsonPrimitive(it))) }, enabled = enabled, groups = groups, createdAt = "2026-11-01T10:00:00Z")

    @Test
    fun aDocumentOpenedByAnAgentCarriesItsAccountDocumentAndCollection() {
        val event = Event("a", 3, "open_outline", JsonObject(mapOf("title" to JsonPrimitive("Rota"), "url" to JsonPrimitive("https://docs.example.org/doc/r1"), "integration_id" to JsonPrimitive("s1"), "document_id" to JsonPrimitive("d-rota"), "collection_id" to JsonPrimitive("col-1"))))

        assertEquals(OutlinePage("Rota", "https://docs.example.org/doc/r1", "s1", "d-rota", "col-1"), outlinePageFrom(event))
    }

    @Test
    fun onlyEnabledOutlineAccountsAvailableToTheGroupAreOffered() {
        val servers = listOf(server("everywhere"), server("ours", groups = listOf("g1")), server("theirs", groups = listOf("g2")), server("off", enabled = false), server("other", app = null))

        assertEquals(listOf("everywhere", "ours"), outlineIntegrations(servers, "g1").map { it.id })
        assertEquals(listOf("everywhere"), outlineIntegrations(servers, null).map { it.id })
    }

    @Test
    fun rowsListOnlyChildrenOfExpandedDocumentsAtTheirDepth() {
        val rows = outlineRows(listOf(engineering, norms), setOf("d-eng"))

        assertEquals(listOf("Engineering" to 1, "On-call" to 2, "Team norms" to 1), rows.map { it.node.title to it.level })
        assertEquals(listOf(true, false, null), rows.map { it.expanded })
    }

    @Test
    fun aDocumentsAncestorsAreItsParentsFromTheTopAndUnknownDocumentsHaveNone() {
        assertEquals(listOf("d-eng", "d-oncall"), outlineAncestors(listOf(engineering, norms), "d-rota"))
        assertEquals(emptyList(), outlineAncestors(listOf(engineering, norms), "d-norms"))
        assertNull(outlineAncestors(listOf(engineering, norms), "d-missing"))
    }

    @Test
    fun aRowOpensAtTheAccountsOriginWhateverPathTheAccountUses() {
        val page = outlinePageOf(rota, server("s1"), "col-1")

        assertEquals(OutlinePage("Rota", "https://docs.example.org/doc/rota-r1", "s1", "d-rota", "col-1"), page)
    }
}
