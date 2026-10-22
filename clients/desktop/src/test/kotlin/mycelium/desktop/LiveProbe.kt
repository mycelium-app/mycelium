package mycelium.desktop

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import mycelium.api.CreateConversationRequest
import mycelium.api.MyceliumApi
import mycelium.state.Store
import java.io.File
import kotlin.test.Test

class LiveProbe {
    @Test
    fun probe() = runBlocking {
        val api = MyceliumApi("http://127.0.0.1:18097", File("/tmp/claude-1000/-home-claude--local-share-orchestrator-projects-mycelium/56d8aa5a-6ff7-4fdf-8ea6-8d0549673abf/scratchpad/devstack/probe.token").readText().trim())
        val store = Store(api, CoroutineScope(Dispatchers.Default))
        store.start()
        delay(1500)
        val node = api.nodes().first().id
        val t0 = System.currentTimeMillis()
        val c = api.createConversation(CreateConversationRequest(node, "claude", "/tmp/claude-1000/-home-claude--local-share-orchestrator-projects-mycelium/56d8aa5a-6ff7-4fdf-8ea6-8d0549673abf/scratchpad/devstack/home/projects/demo", prompt = "first"))
        store.open(c.id)
        var last = -1
        while (System.currentTimeMillis() - t0 < 8000) {
            val n = store.state.value.events[c.id]?.size ?: 0
            if (n != last) { println("PROBE ${System.currentTimeMillis() - t0} ms events=$n status=${store.state.value.conversations[c.id]?.status} connected=${store.state.value.connected}"); last = n }
            delay(20)
        }
    }
}
