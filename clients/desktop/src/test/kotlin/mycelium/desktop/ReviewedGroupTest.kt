package mycelium.desktop

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import mycelium.api.Group
import mycelium.ui.reviewedGroup

private val Slop = Group("g1", "slop", createdAt = "2026-10-22T10:00:00Z")
private val Work = Group("g2", "Work", createdAt = "2026-10-22T10:00:00Z")

class ReviewedGroupTest {
    @Test
    fun aLayerNamedWithDifferentCaseOpensThatGroup() {
        assertEquals(Work, reviewedGroup("work", listOf(Slop, Work), null))
    }

    @Test
    fun aLayerGivenAsTheGroupIdOpensThatGroup() {
        assertEquals(Slop, reviewedGroup("g1", listOf(Slop, Work), null))
    }

    @Test
    fun noLayerOpensTheGroupOfTheConversation() {
        assertEquals(Work, reviewedGroup(null, listOf(Slop, Work), "g2"))
    }

    @Test
    fun noLayerInAConversationWithoutAGroupOpensGlobalInstructions() {
        assertNull(reviewedGroup(null, listOf(Slop, Work), null))
    }

    @Test
    fun theGlobalLayerOpensGlobalInstructions() {
        assertNull(reviewedGroup("Global", listOf(Slop, Work), "g1"))
    }
}
