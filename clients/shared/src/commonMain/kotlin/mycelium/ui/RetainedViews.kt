package mycelium.ui

import androidx.compose.runtime.staticCompositionLocalOf

val LocalRetainedViewKey = staticCompositionLocalOf<String?> { null }

class RetainedViews<T : Any>(private val limit: Int, private val dispose: (T) -> Unit) {
    private val held = LinkedHashMap<String, T>()

    fun obtain(key: String, create: () -> T): T {
        val view = held.remove(key) ?: create()

        held[key] = view

        while (held.size > limit) {
            val oldest = held.keys.first()
            val evicted = held.remove(oldest)

            evicted?.let(dispose)
        }

        return view
    }
}
