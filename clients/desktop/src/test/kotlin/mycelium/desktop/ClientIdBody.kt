package mycelium.desktop

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.assertTrue

fun withoutClientId(body: String): String {
    val sent = Json.parseToJsonElement(body).jsonObject

    assertTrue(sent["client_id"]?.jsonPrimitive?.content?.isNotBlank() == true, "client_id is missing from $body")

    return Json.encodeToString(JsonObject.serializer(), JsonObject(sent - "client_id"))
}
