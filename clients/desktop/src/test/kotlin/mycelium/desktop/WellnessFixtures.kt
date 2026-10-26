package mycelium.desktop

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import mycelium.api.MyceliumApi

internal const val WellnessSettingsJson = """{"break_reminders":true,"break_after_minutes":50,"snooze_minutes":10,"break_minutes":5,"pomodoro":false,"quiet_hours":true,
"quiet_hold_notifications":true,"quiet_hold_schedules":true,"quiet_hold_pings":true,"time_zone":"Europe/London","quiet_days":[
{"weekday":0,"enabled":true,"all_day":false,"starts":"22:00","ends":"08:00"},{"weekday":1,"enabled":true,"all_day":false,"starts":"22:00","ends":"08:00"},
{"weekday":2,"enabled":true,"all_day":false,"starts":"22:00","ends":"08:00"},{"weekday":3,"enabled":true,"all_day":false,"starts":"22:00","ends":"08:00"},
{"weekday":4,"enabled":false,"all_day":false,"starts":"22:00","ends":"08:00"},{"weekday":5,"enabled":true,"all_day":true,"starts":"22:00","ends":"08:00"},
{"weekday":6,"enabled":true,"all_day":true,"starts":"22:00","ends":"08:00"}]}"""

internal val WellnessOverviewJson = """{"days":[
{"day":"2026-10-20","viewing_minutes":95,"agent_minutes":140,"breaks":2,"puzzles":["wordle"]},
{"day":"2026-10-21","viewing_minutes":120,"agent_minutes":200,"breaks":1,"puzzles":[]},
{"day":"2026-10-22","viewing_minutes":0,"agent_minutes":0,"breaks":0,"puzzles":[]},
{"day":"2026-10-23","viewing_minutes":60,"agent_minutes":75,"breaks":3,"puzzles":["wordle","connections"]},
{"day":"2026-10-24","viewing_minutes":30,"agent_minutes":45,"breaks":0,"puzzles":[]},
{"day":"2026-10-25","viewing_minutes":10,"agent_minutes":20,"breaks":0,"puzzles":[]},
{"day":"2026-10-26","viewing_minutes":135,"agent_minutes":190,"breaks":2,"puzzles":["wordle"]}],
"last_break_at":"2026-10-26T10:00:00Z","current_break":null,"quiet_now":false,"held":0}"""

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
                path.endsWith("/wellness/settings") -> respond(WellnessSettingsJson, HttpStatusCode.OK, json)
                path.endsWith("/wellness/overview") -> respond(WellnessOverviewJson, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }

        return MyceliumApi("https://mycelium.test", "token", engine)
    }
}
