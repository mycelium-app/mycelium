package mycelium.state

import kotlin.io.encoding.Base64
import kotlin.time.Instant
import mycelium.api.ActionItem
import mycelium.api.ActionItemRun

enum class ActionItemPhase { Waiting, Running, Succeeded, Failed, Stopped, Done }

private const val OutputTailLines = 200

private val AnsiSequence = Regex("\u001B\\[[0-?]*[ -/]*[@-~]|\u001B\\][^\u0007\u001B]*(?:\u0007|\u001B\\\\)|\u001B[@-Z\\\\-_]|\u001B[()][0-9A-Za-z]")

fun cleanTerminalText(raw: String): String {
    val stripped = AnsiSequence.replace(raw, "").replace("\r\n", "\n")
    val lines = stripped.split('\n').map { line -> line.substringAfterLast('\r') }

    return lines.takeLast(OutputTailLines).joinToString("\n").trimEnd('\n')
}

fun TerminalFeed.cleanText(): String {
    val bytes = chunks.map { Base64.decode(it) }
    val joined = ByteArray(bytes.sumOf { it.size })
    var at = 0

    for (chunk in bytes) {
        chunk.copyInto(joined, at)
        at += chunk.size
    }

    return cleanTerminalText(joined.decodeToString())
}

fun formatElapsed(seconds: Long): String {
    val whole = seconds.coerceAtLeast(0)

    return when {
        whole < 60 -> "${whole}s"
        whole < 3600 -> "${whole / 60}m ${whole % 60}s"
        else -> "${whole / 3600}h ${whole % 3600 / 60}m"
    }
}

fun spokenElapsed(seconds: Long): String {
    val whole = seconds.coerceAtLeast(0)
    val minutes = whole / 60
    val rest = whole % 60

    fun unit(count: Long, name: String) = if (count == 1L) "$count $name" else "$count ${name}s"

    return when {
        minutes == 0L -> unit(rest, "second")
        rest == 0L -> unit(minutes, "minute")
        else -> "${unit(minutes, "minute")} ${unit(rest, "second")}"
    }
}

fun ActionItemRun.elapsedSeconds(now: Instant): Long = (now - Instant.parse(startedAt)).inWholeSeconds

fun ActionItem.phase(exitCodes: Map<String, Int>): ActionItemPhase {
    val run = run ?: return ActionItemPhase.Waiting

    return when (run.status) {
        "running" -> if (exitCodes[run.terminalId] == 0) ActionItemPhase.Succeeded else ActionItemPhase.Running
        "succeeded" -> ActionItemPhase.Succeeded
        "stopped" -> ActionItemPhase.Stopped
        else -> ActionItemPhase.Failed
    }
}
