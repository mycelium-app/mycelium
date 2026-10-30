package mycelium.state

import kotlinx.serialization.json.JsonObject
import mycelium.api.Event
import mycelium.api.EventKinds

private val GrantingOptions = setOf("always", "allow_always")
private const val RevokedOption = "revoked"

fun sessionGrants(events: List<Event>): List<String> {
    val titles = mutableMapOf<String, String>()
    val granted = linkedSetOf<String>()

    for (event in events) {
        val payload = event.payload as? JsonObject ?: continue
        val requestId = payload.string("request_id") ?: continue

        when (event.kind) {
            EventKinds.PermissionRequest -> (payload["toolCall"] as? JsonObject)?.string("title")?.let { titles[requestId] = it }
            EventKinds.PermissionResolved -> when (payload.string("option_id")) {
                in GrantingOptions -> granted += requestId
                RevokedOption -> granted -= requestId
            }
        }
    }

    return granted.mapNotNull { titles[it] }.distinct()
}
