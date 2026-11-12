package mycelium.state

import kotlin.reflect.KClass

class HeldState {
    val held = mutableMapOf<Pair<String, KClass<*>>, Any>()

    inline fun <reified T : Any> of(owner: String, create: () -> T): T {
        val key = owner to T::class

        @Suppress("UNCHECKED_CAST")
        return held.getOrPut(key) { create() } as T
    }
}
