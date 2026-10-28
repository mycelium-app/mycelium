package mycelium

import kotlin.test.Test
import kotlin.test.assertEquals
import mycelium.api.Environment
import mycelium.api.MyceliumJson
import mycelium.state.browserTaskSeconds
import mycelium.state.browserTaskStepsTotal
import mycelium.state.browserTaskStopped
import mycelium.state.environmentActivity

private fun environment(task: String) = MyceliumJson.decodeFromString<Environment>(
    """{"conversation_id":"c1","state":"running","controller":"agent","node_id":"n1","node_name":"atlas-box","size":{"width":1280,"height":800},"started_at":"2026-10-28T10:00:00Z","browser_task":$task}""",
)

class BrowserTaskTest {
    @Test
    fun aRunningTaskIsTheEnvironmentsActivity() {
        val running = environment("""{"goal":"Find flights","state":"running","provider":"OpenJev","steps":[{"label":"Click: From","millis":300}]}""")
        val stopped = environment("""{"goal":"Find flights","state":"stopped","provider":"OpenJev","stopped_by":"took_over"}""")

        assertEquals("Browser task", environmentActivity(running))
        assertEquals("Agent in control", environmentActivity(stopped))
    }

    @Test
    fun stepTimesAreShownInTenthsOfASecond() {
        val task = environment("""{"goal":"g","state":"done","provider":"OpenJev","steps":[{"label":"a","millis":300},{"label":"b","millis":400}]}""").browserTask!!

        assertEquals("0.3 s", browserTaskSeconds(300))
        assertEquals("0.7 s", browserTaskStepsTotal(task))
    }

    @Test
    fun aTaskStoppedByATakeOverSaysSoAndHowFarItGot() {
        val task = environment("""{"goal":"g","state":"stopped","provider":"OpenJev","stopped_by":"took_over","steps":[{"label":"a","millis":1},{"label":"b","millis":1},{"label":"c","millis":1},{"label":"d","millis":1}]}""").browserTask!!

        assertEquals("Stopped when you took over, after 4 steps.", browserTaskStopped(task))
    }
}
