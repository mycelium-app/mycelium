package mycelium

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.test.runTest
import mycelium.state.catching
import mycelium.state.instantOrNull
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.time.Instant

class CallsTest {
    @Test
    fun aTimestampThatCannotBeReadIsNull() {
        assertNull(instantOrNull("yesterday"))
        assertEquals(Instant.parse("2026-10-23T12:00:00Z"), instantOrNull("2026-10-23T12:00:00Z"))
    }

    @Test
    fun aFailedCallIsReturnedAsAFailure() = runTest {
        val result = catching<String> { error("down") }

        assertTrue(result.isFailure)
        assertEquals("down", result.exceptionOrNull()?.message)
    }

    @Test
    fun aCancelledCallIsRethrown() = runTest {
        assertFailsWith<CancellationException> { catching<String> { throw CancellationException("stop") } }
    }
}
