package mycelium

import kotlin.test.Test
import kotlin.test.assertEquals
import mycelium.state.spokenVersion
import mycelium.state.withoutSpoken

class SpokenTest {
    private val reply = "Two tests fail:\n\n- picker\n- wording\n\n```mycelium-spoken\nTwo tests still fail.\n```"

    @Test
    fun theSpokenVersionIsTheBlockAlone() {
        assertEquals("Two tests still fail.", spokenVersion(reply))
    }

    @Test
    fun aReplyWithoutABlockIsSpokenWhole() {
        assertEquals("All tests pass.", spokenVersion("All tests pass."))
    }

    @Test
    fun anEmptyBlockFallsBackToTheWholeReply() {
        assertEquals("Done.\n\n```mycelium-spoken\n```", spokenVersion("Done.\n\n```mycelium-spoken\n```"))
    }

    @Test
    fun copyingLeavesTheBlockOutAndKeepsOtherCode() {
        assertEquals("Run this:\n\n```shell\npnpm test\n```", withoutSpoken("Run this:\n\n```shell\npnpm test\n```\n\n```mycelium-spoken\nRun the tests.\n```"))
    }
}
