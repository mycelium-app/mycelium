package mycelium.ui

import androidx.compose.runtime.staticCompositionLocalOf

val LocalRetainedViewKey = staticCompositionLocalOf<String?> { null }

class RetainedViews<T : Any>(private val limit: Int, private val dispose: (T) -> Unit) {
    private val held = LinkedHashMap<String, T>()
    private val onScreen = mutableMapOf<String, Int>()
    private val forgotten = mutableSetOf<String>()

    fun obtain(key: String, create: () -> T): T {
        val view = held.remove(key) ?: create()

        held[key] = view
        onScreen[key] = (onScreen[key] ?: 0) + 1
        forgotten -= key
        trim()

        return view
    }

    fun release(key: String) {
        val remaining = (onScreen[key] ?: 0) - 1

        if (remaining > 0) {
            onScreen[key] = remaining
            return
        }

        onScreen -= key

        if (forgotten.remove(key)) {
            held.remove(key)?.let(dispose)
        }

        trim()
    }

    fun forget(key: String) {
        if (key in onScreen) {
            forgotten += key
            return
        }

        held.remove(key)?.let(dispose)
    }

    private fun trim() {
        while (held.size > limit) {
            val oldest = held.keys.firstOrNull { it !in onScreen } ?: return

            held.remove(oldest)?.let(dispose)
        }
    }
}
