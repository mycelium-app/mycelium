package mycelium.desktop

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.MockEngineConfig
import io.ktor.client.engine.mock.MockRequestHandler
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers

class SynchronousMockEngine(handler: MockRequestHandler) : MockEngine(MockEngineConfig().apply { addHandler(handler) }) {
    override val dispatcher: CoroutineDispatcher = Dispatchers.Unconfined
}
