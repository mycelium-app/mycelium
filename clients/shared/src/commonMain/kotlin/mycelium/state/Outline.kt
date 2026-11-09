package mycelium.state

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive
import mycelium.api.ChatToolServer
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.OutlineDocumentNode

data class OutlinePage(val title: String, val url: String, val integrationId: String? = null, val documentId: String? = null, val collectionId: String? = null)

data class OutlineRow(val node: OutlineDocumentNode, val level: Int, val expanded: Boolean?)

fun outlinePageFrom(event: Event): OutlinePage? {
    val payload = event.payload as? JsonObject ?: return null

    return if (event.kind == EventKinds.OpenOutline) OutlinePage(payload.string("title").orEmpty(), payload.string("url") ?: return null, payload.string("integration_id"), payload.string("document_id"), payload.string("collection_id")) else null
}

fun outlineIntegrations(servers: List<ChatToolServer>, groupId: String?): List<ChatToolServer> =
    servers.filter { server -> server.enabled && server.settings?.get("app")?.jsonPrimitive?.content == "outline" && (server.groups.isEmpty() || groupId in server.groups) }

fun outlineRows(nodes: List<OutlineDocumentNode>, expanded: Set<String>, level: Int = 1): List<OutlineRow> = nodes.flatMap { node ->
    val open = node.id in expanded
    val row = OutlineRow(node, level, if (node.children.isEmpty()) null else open)

    if (open) listOf(row) + outlineRows(node.children, expanded, level + 1) else listOf(row)
}

fun outlineAncestors(nodes: List<OutlineDocumentNode>, documentId: String): List<String>? {
    for (node in nodes) {
        if (node.id == documentId) {
            return emptyList()
        }

        val below = outlineAncestors(node.children, documentId)

        if (below != null) {
            return listOf(node.id) + below
        }
    }

    return null
}

fun outlineDocumentIds(nodes: List<OutlineDocumentNode>): Set<String> = nodes.flatMap { outlineDocumentIds(it.children) + it.id }.toSet()

fun outlinePageOf(node: OutlineDocumentNode, server: ChatToolServer, collectionId: String): OutlinePage {
    val origin = server.url.substringBefore("://") + "://" + server.url.substringAfter("://").substringBefore('/')

    return OutlinePage(node.title, origin + node.url, server.id, node.id, collectionId)
}
