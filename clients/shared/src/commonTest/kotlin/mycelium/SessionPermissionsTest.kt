package mycelium

import kotlinx.serialization.json.Json
import mycelium.api.Event
import mycelium.state.sessionGrants
import kotlin.test.Test
import kotlin.test.assertEquals

class SessionPermissionsTest {
    private fun event(seq: Long, kind: String, payload: String) = Event("c1", seq, kind, Json.parseToJsonElement(payload))

    private fun request(seq: Long, requestId: String, title: String) = event(seq, "permission_request", """{"request_id":"$requestId","toolCall":{"title":"$title"}}""")

    private fun resolved(seq: Long, requestId: String, option: String) = event(seq, "permission_resolved", """{"request_id":"$requestId","option_id":"$option"}""")

    @Test
    fun aToolApprovedForTheSessionIsListedOnce() {
        val events = listOf(request(1, "a", "terminal_type"), resolved(2, "a", "always"), request(3, "b", "terminal_type"), resolved(4, "b", "allow_always"))

        assertEquals(listOf("terminal_type"), sessionGrants(events))
    }

    @Test
    fun aSingleApprovalIsNotAGrant() {
        val events = listOf(request(1, "a", "terminal_type"), resolved(2, "a", "approve"), request(3, "b", "terminal_read"), resolved(4, "b", "deny"))

        assertEquals(emptyList(), sessionGrants(events))
    }

    @Test
    fun aRevokedGrantIsNoLongerListed() {
        val events = listOf(request(1, "a", "terminal_type"), resolved(2, "a", "always"), resolved(3, "a", "revoked"))

        assertEquals(emptyList(), sessionGrants(events))
    }

    @Test
    fun approvingAgainAfterARevokeGrantsItAgain() {
        val events = listOf(request(1, "a", "terminal_type"), resolved(2, "a", "always"), resolved(3, "a", "revoked"), request(4, "b", "terminal_type"), resolved(5, "b", "always"))

        assertEquals(listOf("terminal_type"), sessionGrants(events))
    }
}
