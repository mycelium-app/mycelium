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
        views.release("a")
        views.obtain("b") { "B" }
        views.release("b")
        views.obtain("a") { "A2" }
        views.release("a")
        views.obtain("c") { "C" }

        assertEquals(listOf("B"), disposed)
    }

    @Test
    fun a_view_on_screen_is_never_disposed_to_make_room() {
        views.obtain("a") { "A" }
        views.obtain("b") { "B" }
        views.obtain("c") { "C" }

        assertEquals(emptyList(), disposed)
    }

    @Test
    fun a_released_view_is_kept_until_the_idle_views_go_over_the_limit() {
        views.obtain("a") { "A" }
        views.obtain("b") { "B" }
        views.release("a")
        views.release("b")
        views.obtain("c") { "C" }

        assertEquals(listOf("A"), disposed)
    }

    @Test
    fun forgetting_an_idle_view_disposes_it_at_once() {
        views.obtain("a") { "A" }
        views.release("a")

        views.forget("a")

        assertEquals(listOf("A"), disposed)
    }

    @Test
    fun forgetting_a_view_on_screen_disposes_it_when_it_is_released_instead_of_keeping_it() {
        views.obtain("a") { "A" }

        views.forget("a")
        assertEquals(emptyList(), disposed)
        views.release("a")

        assertEquals(listOf("A"), disposed)
        var created = 0
        views.obtain("a") { created += 1; "A2" }
        assertEquals(1, created)
    }
}
