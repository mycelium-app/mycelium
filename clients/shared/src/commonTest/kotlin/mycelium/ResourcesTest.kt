package mycelium

import mycelium.api.CpuPoint
import mycelium.api.ListeningPort
import mycelium.api.NodeSummary
import mycelium.api.ResourceSample
import mycelium.api.StreamMessage
import mycelium.api.parseStreamMessage
import mycelium.state.AppState
import mycelium.state.ResourceHistoryLimit
import mycelium.state.cpuSummary
import mycelium.state.formatBytes
import mycelium.state.percentOf
import mycelium.state.portLink
import mycelium.state.reduce
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull

private fun node(online: Boolean = true, address: String = "10.0.0.5") = NodeSummary("n1", "willow", null, "willow", "linux", "x86_64", "v1", address, null, online, false)

private fun sample(at: Long, cpu: Double = 10.0) = ResourceSample(atMs = at, available = true, cpuPercent = cpu)

class ResourcesTest {
    @Test
    fun bytesAreWrittenInDecimalUnitsWithOneDecimalUnderOneHundred() {
        assertEquals("640 B", formatBytes(640))
        assertEquals("1.5 KB", formatBytes(1500))
        assertEquals("21.4 GB", formatBytes(21_400_000_000))
        assertEquals("140 GB", formatBytes(140_000_000_000))
    }

    @Test
    fun aShareOfNothingIsZeroPercent() {
        assertEquals(0, percentOf(5, 0))
        assertEquals(66, percentOf(21, 32))
    }

    @Test
    fun aWebPortOnAReachableAddressLinksToTheNodesAddress() {
        assertEquals("http://10.0.0.5:3000", portLink(node(), ListeningPort(3000, "0.0.0.0")))
        assertEquals("https://10.0.0.5:8443", portLink(node(), ListeningPort(8443, "::")))
        assertEquals("http://[fd00::5]:3000", portLink(node(address = "fd00::5"), ListeningPort(3000, "0.0.0.0")))
    }

    @Test
    fun aPortThatIsNotWebLikeLoopbackOnlyOrOnAnOfflineNodeHasNoLink() {
        assertNull(portLink(node(), ListeningPort(5432, "0.0.0.0")))
        assertNull(portLink(node(), ListeningPort(3000, "127.0.0.1")))
        assertNull(portLink(node(), ListeningPort(3000, "::1")))
        assertNull(portLink(node(online = false), ListeningPort(3000, "0.0.0.0")))
        assertNull(portLink(node(address = ""), ListeningPort(3000, "0.0.0.0")))
    }

    @Test
    fun cpuIsSummarisedAsAverageAndPeak() {
        assertNull(cpuSummary(emptyList()))
        assertEquals("Average 40%, peak 70%", cpuSummary(listOf(CpuPoint(0, 10.0), CpuPoint(1, 70.0))))
    }

    @Test
    fun aStreamedSampleBecomesTheLatestAndExtendsTheHistoryOnce() {
        val first = reduce(AppState(), StreamMessage.NodeResourcesSampled("n1", sample(1000, 10.0)))
        val again = reduce(first, StreamMessage.NodeResourcesSampled("n1", sample(1000, 10.0)))
        val next = reduce(again, StreamMessage.NodeResourcesSampled("n1", sample(4000, 30.0)))

        assertEquals(4000, next.resources["n1"]?.latest?.atMs)
        assertEquals(listOf(1000L, 4000L), next.resources["n1"]?.history?.map { it.atMs })
    }

    @Test
    fun theHistoryKeepsOnlyTheNewestSamples() {
        var state = AppState()

        for (at in 1L..ResourceHistoryLimit + 5L) {
            state = reduce(state, StreamMessage.NodeResourcesSampled("n1", sample(at)))
        }

        assertEquals(ResourceHistoryLimit, state.resources["n1"]?.history?.size)
        assertEquals(6L, state.resources["n1"]?.history?.first()?.atMs)
    }

    @Test
    fun revokingANodeForgetsItsSamples() {
        val sampled = reduce(AppState(nodes = mapOf("n1" to node())), StreamMessage.NodeResourcesSampled("n1", sample(1)))

        assertEquals(emptyMap(), reduce(sampled, StreamMessage.NodeRemoved("n1")).resources)
    }

    @Test
    fun aNodeResourcesFrameIsParsedFromTheStream() {
        val frame = """{"type":"node_resources","node_id":"n1","sample":{"at_ms":5,"available":true,"cpu_percent":12.5,"cores":4,"load":[0.1,0.2,0.3],
            "ports":[{"port":3000,"address":"0.0.0.0","process":"node"}]}}"""

        val message = parseStreamMessage(frame)

        assertIs<StreamMessage.NodeResourcesSampled>(message)
        assertEquals(12.5, message.sample.cpuPercent)
        assertEquals("node", message.sample.ports.single().process)
    }
}
