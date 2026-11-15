package mycelium.state

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import mycelium.ClientVersion
import mycelium.api.ClientEvent
import mycelium.api.ClientEventReport
import kotlin.time.Clock
import kotlin.time.Instant

private const val FlushMillis = 10_000L
private const val SpacingMillis = 3_000L
private const val LargestBatch = 50
private const val LargestBuffer = 200
private const val LongestMessage = 2_000

class ClientReporter(
    private val client: String,
    private val sessionId: String,
    scope: CoroutineScope,
    private val send: suspend (ClientEventReport) -> Unit,
    private val now: () -> Instant = { Clock.System.now() },
) {
    private val pending = Channel<ClientEvent>(LargestBuffer, BufferOverflow.DROP_OLDEST)

    init {
        scope.launch {
            while (true) {
                val batch = mutableListOf(pending.receive())

                withTimeoutOrNull(FlushMillis) {
                    while (batch.size < LargestBatch) {
                        batch += pending.receive()
                    }
                }

                try {
                    send(ClientEventReport(sessionId, client, ClientVersion, batch))
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (_: Exception) {
                }

                delay(SpacingMillis)
            }
        }
    }

    fun report(kind: String, message: String, detail: Map<String, Any?> = emptyMap()) {
        val fields = detail.mapNotNull { (name, value) ->
            when (value) {
                null -> null
                is Number -> name to JsonPrimitive(value)
                is Boolean -> name to JsonPrimitive(value)
                else -> name to JsonPrimitive(value.toString())
            }
        }.toMap()

        pending.trySend(ClientEvent(now().toString(), kind, message.take(LongestMessage), JsonObject(fields)))
    }
}

fun newRequestId(): String {
    val bytes = kotlin.random.Random.nextBytes(16)
    bytes[6] = ((bytes[6].toInt() and 0x0f) or 0x40).toByte()
    bytes[8] = ((bytes[8].toInt() and 0x3f) or 0x80).toByte()
    val hex = bytes.joinToString("") { byte -> (byte.toInt() and 0xff).toString(16).padStart(2, '0') }

    return "${hex.substring(0, 8)}-${hex.substring(8, 12)}-${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}"
}

fun clientKind(os: String): String = when (os) {
    "web" -> "web"
    "android" -> "android"
    else -> "desktop"
}
