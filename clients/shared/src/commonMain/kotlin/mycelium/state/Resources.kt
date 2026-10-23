package mycelium.state

import mycelium.api.CpuPoint
import mycelium.api.ListeningPort
import mycelium.api.NodeSummary
import mycelium.api.ResourceSample
import kotlin.math.roundToInt

private val HttpPorts = setOf(80, 443, 3000, 3001, 4000, 4200, 5000, 5173, 8000, 8008, 8080, 8081, 8088, 8443, 8888, 9000)
private val SecurePorts = setOf(443, 8443)
private val LoopbackAddresses = setOf("127.0.0.1", "::1")

fun formatBytes(bytes: Long): String {
    val units = listOf("B", "KB", "MB", "GB", "TB")
    var value = bytes.toDouble()
    var unit = 0

    while (value >= 1000 && unit < units.lastIndex) {
        value /= 1000
        unit++
    }

    if (unit == 0) {
        return "$bytes B"
    }

    val tenths = (value * 10).roundToInt()

    return if (tenths >= 1000 || tenths % 10 == 0) "${(tenths + 5) / 10} ${units[unit]}" else "${tenths / 10}.${tenths % 10} ${units[unit]}"
}

fun percentOf(part: Long, whole: Long): Int = if (whole <= 0) 0 else ((part.toDouble() / whole) * 100).roundToInt().coerceIn(0, 100)

fun memoryUsedBytes(sample: ResourceSample): Long = (sample.memoryTotalBytes - sample.memoryAvailableBytes).coerceAtLeast(0)

fun looksLikeHttp(port: ListeningPort): Boolean = port.port in HttpPorts

fun isLocalToNode(port: ListeningPort): Boolean = port.address in LoopbackAddresses

fun portLink(node: NodeSummary, port: ListeningPort): String? {
    if (!node.online || node.address.isEmpty() || isLocalToNode(port) || !looksLikeHttp(port)) {
        return null
    }

    val host = if (node.address.contains(':')) "[${node.address}]" else node.address
    val scheme = if (port.port in SecurePorts) "https" else "http"

    return "$scheme://$host:${port.port}"
}

fun cpuSummary(history: List<CpuPoint>): String? {
    if (history.isEmpty()) {
        return null
    }

    val average = history.map { it.cpuPercent }.average().roundToInt()
    val peak = history.maxOf { it.cpuPercent }.roundToInt()

    return "Average ${average}%, peak ${peak}%"
}
