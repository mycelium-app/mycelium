package mycelium.desktop

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.rules.TestWatcher
import org.junit.runner.Description

class TestScopes : TestWatcher() {
    private val created = mutableListOf<CoroutineScope>()

    fun create(): CoroutineScope {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        created += scope

        return scope
    }

    override fun finished(description: Description) {
        created.forEach { it.cancel() }
        created.clear()
    }
}
