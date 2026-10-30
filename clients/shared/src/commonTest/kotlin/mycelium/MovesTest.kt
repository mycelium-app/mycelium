package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.MovePlan
import mycelium.api.MoveRequest
import mycelium.api.MoveState
import mycelium.api.MyceliumApi
import mycelium.api.StreamMessage
import mycelium.api.parseStreamMessage
import mycelium.state.AppState
import mycelium.state.reduce
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull

private const val MoveJson = """{"type":"move","conversation_id":"c1","node_id":"n2","fork":false,"state":"waiting"}"""

class MovesTest {
    @Test
    fun aMoveMessageOnTheStreamIsKeptPerConversation() {
        val message = parseStreamMessage(MoveJson)

        assertIs<StreamMessage.MoveChanged>(message)
        val state = reduce(AppState(), message)

        assertEquals(MoveState.Waiting, state.moves.getValue("c1").state)
        assertEquals(MoveState.Done, reduce(state, parseStreamMessage(MoveJson.replace("waiting", "done"))!!).moves.getValue("c1").state)
    }

    @Test
    fun theMoveCallsUseTheConversationsMoveAddresses() = runTest {
        val seen = mutableListOf<Triple<String, String, String>>()
        val engine = MockEngine { request ->
            seen += Triple(request.method.value, request.url.encodedPath + (request.url.parameters["node_id"]?.let { "?node_id=$it" } ?: ""), String(request.body.toByteArray()))
            val body = when {
                request.url.encodedPath.endsWith("/move-plan") -> """{"movable":true,"branch":"main","changed_files":2,"project_on_target":false,"clones_project":true,"agent_session":true,"stays_ports":[5173],"stays_processes":["node"]}"""
                request.method == HttpMethod.Delete -> ""
                request.method == HttpMethod.Get -> "null"
                else -> """{"conversation_id":"c1","node_id":"n2","fork":true,"state":"waiting"}"""
            }

            respond(body, if (request.method == HttpMethod.Delete) HttpStatusCode.NoContent else HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val api = MyceliumApi("https://mycelium.test", "token", engine)

        val planned = api.movePlan("c1", "n2")
        val requested = api.requestMove("c1", MoveRequest("n2", fork = true))
        val current = api.currentMove("c1")
        api.cancelMove("c1")

        assertEquals(MovePlan(true, null, "main", 2, false, true, true, listOf(5173), listOf("node")), planned)
        assertEquals(MoveState.Waiting, requested.state)
        assertNull(current)
        assertEquals(
            listOf(
                Triple("GET", "/api/v1/conversations/c1/move-plan?node_id=n2", ""),
                Triple("POST", "/api/v1/conversations/c1/move", """{"node_id":"n2","fork":true}"""),
                Triple("GET", "/api/v1/conversations/c1/move", ""),
                Triple("DELETE", "/api/v1/conversations/c1/move", ""),
            ),
            seen,
        )
    }
}
