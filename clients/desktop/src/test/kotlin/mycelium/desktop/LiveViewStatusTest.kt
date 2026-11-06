package mycelium.desktop

import kotlin.test.Test
import kotlin.test.assertEquals

class LiveViewStatusTest {
    @Test
    fun onlyTheLiveViewsDisconnectedStatusCountsAsADisconnect() {
        val requests = listOf(
            """{"type":"status","state":"disconnected","clean":false}""",
            """{"type":"status","state":"connected"}""",
            """{"type":"pointer"}""",
            "document.title",
        )

        assertEquals(listOf(true, false, false, false), requests.map(::isDisconnectedStatus))
    }
}
