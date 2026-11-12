package mycelium

import mycelium.ui.RetainedViews
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertSame

class RetainedViewsTest {
    private val disposed = mutableListOf<String>()
    private val views = RetainedViews<String>(2) { disposed += it }

    @Test
    fun asking_again_for_a_key_returns_the_view_it_created_instead_of_creating_another() {
        var created = 0
        val first = views.obtain("outline:c1") { created += 1; "view-${created}" }
        val second = views.obtain("outline:c1") { created += 1; "view-${created}" }

        assertSame(first, second)
        assertEquals(1, created)
        assertEquals(emptyList(), disposed)
    }

    @Test
    fun going_over_the_limit_disposes_the_view_used_longest_ago() {
        views.obtain("a") { "A" }
        views.obtain("b") { "B" }
        views.obtain("a") { "A2" }
        views.obtain("c") { "C" }

        assertEquals(listOf("B"), disposed)
    }
}
