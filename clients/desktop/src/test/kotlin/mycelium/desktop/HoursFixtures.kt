package mycelium.desktop

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.runBlocking
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.state.Store
import kotlin.time.Clock
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant

internal class HoursPlatform(private val preference: String) : Platform by QuietPlatform {
    override fun loadPreference(key: String): String? = preference.takeIf { key == "conversation-group-filter" }

    override fun savePreference(key: String, value: String) = Unit

    override fun localDateTime(epochMillis: Long): String? = Instant.fromEpochMilliseconds(epochMillis).toString().take(16)
}

internal fun hoursDaysJson(closed: Boolean, opens: String = "09:00", closes: String = "18:00") =
    (0..6).joinToString(",", "[", "]") { """{"weekday":$it,"closed":$closed,"opens":"$opens","closes":"$closes"}""" }

internal fun hoursGroupJson(id: String, name: String, hours: String = """{"always_open":true,"days":[]}""", openedUntil: String? = null) =
    """{"id":"$id","name":"$name","folders":[],"created_at":"2026-10-22T10:00:00Z","hours":$hours,"open_now":true,"opened_until":${openedUntil?.let { "\"$it\"" }}}"""

internal fun closedAllWeek() = """{"always_open":false,"days":${hoursDaysJson(closed = true)}}"""

internal fun hoursConversationJson(id: String, title: String, group: String?, status: String = "done") =
    """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/$id","title":"$title","status":"$status","acp_session_id":"s","mode":null,"model":null,"group_id":${group?.let { "\"$it\"" }},
"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null,"pinned_at":null}"""

internal fun clockIn(minutes: Int): String = (Clock.System.now() + minutes.minutes).toString().substring(11, 16)

internal fun isoIn(minutes: Int): String = (Clock.System.now() + minutes.minutes).toString()

internal class HoursServer(private val groups: List<String>, private val conversations: List<String>, val settings: String = WellnessSettingsJson.replace("\"always_open\":false", "\"always_open\":true")) {
    val seen = java.util.Collections.synchronizedList(mutableListOf<Triple<String, String, String>>())

    var savedHoursResponse: String = "{}"

    fun sent(method: String, suffix: String) = seen.toList().firstOrNull { it.first == method && it.second.endsWith(suffix) }

    fun store(scope: CoroutineScope, platform: Platform): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            seen += Triple(request.method.value, path, String(request.body.toByteArray()))

            when {
                request.method == HttpMethod.Post && path.endsWith("/open") -> respond("""{"opened_until":"${isoIn(15)}"}""", HttpStatusCode.Created, json)
                request.method == HttpMethod.Put && path.endsWith("/hours") -> respond(savedHoursResponse, HttpStatusCode.OK, json)
                request.method == HttpMethod.Delete && path.endsWith("/open") -> respond("", HttpStatusCode.NoContent)
                path.endsWith("/experiments") -> respond(EnabledExperiments, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond(conversations.joinToString(",", "[", "]"), HttpStatusCode.OK, json)
                path.endsWith("/groups") -> respond(groups.joinToString(",", "[", "]"), HttpStatusCode.OK, json)
                path.endsWith("/wellness/settings") -> respond(settings, HttpStatusCode.OK, json)
                path.endsWith("/wellness/overview") -> respond(WellnessOverviewJson, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond("""[{"id":"n1","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]}]""", HttpStatusCode.OK, json)
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/projects") || path.endsWith("/worktrees") || path.endsWith("/assistants") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scope, platform)

        runBlocking {
            store.refresh()
            store.refreshGroups()
            store.wellness.load()
        }

        return store
    }
}
