package mycelium.state

import kotlinx.serialization.json.JsonObject
import mycelium.api.Event
import mycelium.api.EventKinds

data class OutlinePage(val title: String, val url: String)

fun outlinePageFrom(event: Event): OutlinePage? {
    val payload = event.payload as? JsonObject ?: return null

    return if (event.kind == EventKinds.OpenOutline) OutlinePage(payload.string("title").orEmpty(), payload.string("url") ?: return null) else null
}

private val openedOutline = Regex("^Opened (.*) beside the conversation: (\\S+)$")

fun outlinePageFromOutput(output: String): OutlinePage? =
    openedOutline.matchEntire(output.trim())?.let { OutlinePage(it.groupValues[1], it.groupValues[2]) }
