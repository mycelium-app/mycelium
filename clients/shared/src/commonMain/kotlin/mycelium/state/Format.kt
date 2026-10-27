package mycelium.state

import kotlin.time.Clock
import kotlin.time.Duration
import kotlin.time.Instant

fun agentName(agent: String): String = when (agent) {
    "claude" -> "Claude Code"
    "codex" -> "Codex"
    "chat" -> "Chat"
    else -> agent
}

fun loginMethodName(method: String): String = when (method.replace('_', '-')) {
    "subscription", "device-auth" -> "Subscription sign-in"
    "token", "setup-token", "oauth-token" -> "Subscription token"
    "api-key" -> "API key"
    else -> method.replace('_', ' ').replace('-', ' ').replaceFirstChar { it.uppercase() }
}

fun agentVersion(version: String?): String = version.orEmpty().trim().substringAfterLast(' ')

fun versionWithUpdate(version: String, updateAvailable: String?): String = if (updateAvailable == null) version else "$version · $updateAvailable available"

fun instantOrNull(text: String): Instant? = runCatching { Instant.parse(text) }.getOrNull()

fun relativeTime(iso: String, now: Instant = Clock.System.now()): String {
    val then = instantOrNull(iso) ?: return ""
    val elapsed: Duration = now - then

    return when {
        elapsed.inWholeSeconds < 60 -> "now"
        elapsed.inWholeMinutes < 60 -> "${elapsed.inWholeMinutes}m"
        elapsed.inWholeHours < 24 -> "${elapsed.inWholeHours}h"
        else -> "${elapsed.inWholeDays}d"
    }
}

fun ago(iso: String, now: Instant = Clock.System.now()): String = relativeTime(iso, now).let { if (it == "now") it else "$it ago" }

fun absoluteTime(iso: String, localDateTime: (Long) -> String?): String {
    val moment = instantOrNull(iso) ?: return ""
    val local = localDateTime(moment.toEpochMilliseconds()) ?: iso.substringBefore('Z').take(16)
    val (year, month, day) = local.substringBefore('T').split('-')

    return "${day.toInt()} ${MonthAbbreviations[month.toInt() - 1]} $year ${local.substringAfter('T').take(5)}"
}

private val MonthAbbreviations = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")

fun projectName(cwd: String): String = cwd.trimEnd('/').substringAfterLast('/').ifEmpty { cwd }

fun isLaptop(os: String): Boolean = os.lowercase().let { it.contains("mac") || it.contains("darwin") || it.contains("windows") }

fun until(iso: String, now: Instant = Clock.System.now()): String {
    val then = instantOrNull(iso) ?: return ""
    val remaining: Duration = then - now

    return when {
        remaining.inWholeSeconds < 60 -> "in under a minute"
        remaining.inWholeMinutes < 60 -> "in ${remaining.inWholeMinutes}m"
        remaining.inWholeHours < 24 -> "in ${remaining.inWholeHours}h"
        else -> "in ${remaining.inWholeDays}d"
    }
}

fun elapsed(iso: String, now: Instant = Clock.System.now()): String {
    val then = instantOrNull(iso) ?: return ""
    val total = (now - then).inWholeSeconds.coerceAtLeast(0)

    return when {
        total < 3600 -> "${total / 60}m ${(total % 60).toString().padStart(2, '0')}s"
        else -> "${total / 3600}h ${((total % 3600) / 60).toString().padStart(2, '0')}m"
    }
}
