package mycelium.desktop

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import mycelium.api.MyceliumApi

internal const val WellnessSettingsJson = """{"break_reminders":true,"break_after_minutes":50,"snooze_minutes":10,"break_minutes":5,
"quiet_hold_notifications":true,"quiet_hold_schedules":true,"quiet_hold_pings":true,"time_zone":"Europe/London","hours":{"always_open":false,"days":[
{"weekday":0,"closed":false,"opens":"09:00","closes":"18:00"},{"weekday":1,"closed":false,"opens":"09:00","closes":"18:00"},
{"weekday":2,"closed":false,"opens":"09:00","closes":"18:00"},{"weekday":3,"closed":false,"opens":"09:00","closes":"18:00"},
{"weekday":4,"closed":false,"opens":"09:00","closes":"17:00"},{"weekday":5,"closed":true,"opens":"09:00","closes":"18:00"},
{"weekday":6,"closed":true,"opens":"09:00","closes":"18:00"}]}}"""

internal const val WorkGroupJson = """{"id":"g-work","name":"work","created_at":"2026-10-01T09:00:00Z","hours":{"always_open":false,"days":[
{"weekday":0,"closed":false,"opens":"09:00","closes":"18:00"},{"weekday":1,"closed":false,"opens":"09:00","closes":"18:00"},
{"weekday":2,"closed":false,"opens":"09:00","closes":"18:00"},{"weekday":3,"closed":false,"opens":"09:00","closes":"18:00"},
{"weekday":4,"closed":false,"opens":"09:00","closes":"18:00"},{"weekday":5,"closed":true,"opens":"09:00","closes":"18:00"},
{"weekday":6,"closed":true,"opens":"09:00","closes":"18:00"}]},"open_now":true}"""

internal const val PersonalGroupJson = """{"id":"g-personal","name":"personal","created_at":"2026-10-01T09:00:00Z","hours":{"always_open":true,"days":[]},"open_now":true}"""

internal const val WellnessGroupsJson = "[$WorkGroupJson,$PersonalGroupJson]"

internal val WellnessOverviewJson = """{"days":[
{"day":"2026-10-20","viewing_minutes":95,"agent_minutes":140,"breaks":2},
{"day":"2026-10-21","viewing_minutes":120,"agent_minutes":200,"breaks":1},
{"day":"2026-10-22","viewing_minutes":0,"agent_minutes":0,"breaks":0},
{"day":"2026-10-23","viewing_minutes":60,"agent_minutes":75,"breaks":3},
{"day":"2026-10-24","viewing_minutes":30,"agent_minutes":45,"breaks":0},
{"day":"2026-10-25","viewing_minutes":10,"agent_minutes":20,"breaks":0},
{"day":"2026-10-26","viewing_minutes":135,"agent_minutes":190,"breaks":2}],
"last_break_at":"2026-10-26T10:00:00Z","current_break":null,"quiet_now":false,"held":0,"held_week":{"notifications":11,"schedules":2,"pings":3},
"opened_out_of_hours":[{"group_id":"g-work","group_name":"work","reason":"Restart the release workflow that stalled","opened_at":"2026-10-26T21:32:00Z","until":"2026-10-26T21:47:00Z","closed_at":null}]}"""

internal const val WellnessPuzzlesJson = """[
{"puzzle":"wordle","day":"2026-10-27","today":{"solved":true,"score":4,"seconds":220,"source":"native"},"history":[
{"day":"2026-10-26","result":{"solved":true,"score":3,"source":"native"},"recorded_at":"2026-10-26T10:00:00Z"},
{"day":"2026-10-25","result":{"solved":true,"score":4,"source":"manual"},"recorded_at":"2026-10-25T10:00:00Z"},
{"day":"2026-10-14","result":{"solved":false,"source":"native"},"recorded_at":"2026-10-14T10:00:00Z"},
{"day":"2026-09-12","result":{"solved":true,"score":5,"source":"native"},"recorded_at":"2026-09-12T10:00:00Z"}],
"stats":{"played":22,"solved":21,"average":3.9,"average_seconds":220.0,"streak":12,"best_streak":31,"distribution":[{"value":3,"count":2},{"value":4,"count":9}]}},
{"puzzle":"connections","day":"2026-10-27","today":null,"history":[],"stats":{"played":5,"solved":5,"average":1.0,"streak":5,"best_streak":5,"distribution":[]}},
{"puzzle":"daily-orbs","day":"2026-10-27","today":null,"history":[],"stats":{"played":0,"solved":0,"average":null,"streak":8,"best_streak":14,"distribution":[]}},
{"puzzle":"catfishing","day":"2026-10-27","today":{"score":7,"source":"read"},"history":[],"stats":{"played":3,"solved":0,"average":7.0,"streak":3,"best_streak":3,"distribution":[]}},
{"puzzle":"enclose-horse","day":"2026-10-27","today":{"score":87,"source":"read"},"history":[],"stats":{"played":1,"solved":0,"average":87.0,"streak":1,"best_streak":1,"distribution":[]}}]"""

internal class WellnessRequests {
    val seen = java.util.Collections.synchronizedList(mutableListOf<Triple<String, String, String>>())

    fun sent(method: String, suffix: String) = seen.toList().firstOrNull { it.first == method && it.second.endsWith(suffix) }

    fun api(): MyceliumApi {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            seen += Triple(request.method.value, path, String(request.body.toByteArray()))

            when {
                request.method == HttpMethod.Put && path.endsWith("/wellness/settings") -> respond(String(request.body.toByteArray()), HttpStatusCode.OK, json)
                request.method != HttpMethod.Get && path.contains("/wellness/") && !path.endsWith("/wellness/breaks") -> respond("", HttpStatusCode.NoContent)
                request.method == HttpMethod.Put && path.endsWith("/hours") -> respond(WorkGroupJson, HttpStatusCode.OK, json)
                path.endsWith("/groups") -> respond(WellnessGroupsJson, HttpStatusCode.OK, json)
                path.endsWith("/wellness/settings") -> respond(WellnessSettingsJson, HttpStatusCode.OK, json)
                path.endsWith("/wellness/puzzles") -> respond(WellnessPuzzlesJson, HttpStatusCode.OK, json)
                path.endsWith("/wellness/overview") -> respond(WellnessOverviewJson, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }

        return MyceliumApi("https://mycelium.test", "token", engine)
    }
}
