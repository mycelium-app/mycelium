package mycelium.state

import kotlin.time.Clock
import kotlin.time.Duration
import kotlin.time.Instant

fun agentName(agent: String): String = when (agent) {
    "claude" -> "Claude Code"
    "codex" -> "Codex"
    else -> agent
}

fun loginMethodName(method: String): String = when (method.replace('_', '-')) {
    "subscription", "device-auth" -> "Subscription sign-in"
    "token", "setup-token", "oauth-token" -> "Subscription token"
    "api-key" -> "API key"
    else -> method.replace('_', ' ').replace('-', ' ').replaceFirstChar { it.uppercase() }
}

fun agentVersion(version: String?): String = version.orEmpty().trim().substringAfterLast(' ')

fun relativeTime(iso: String, now: Instant = Clock.System.now()): String {
    val then = runCatching { Instant.parse(iso) }.getOrNull() ?: return ""
    val elapsed: Duration = now - then

    return when {
        elapsed.inWholeSeconds < 60 -> "now"
        elapsed.inWholeMinutes < 60 -> "${elapsed.inWholeMinutes}m"
        elapsed.inWholeHours < 24 -> "${elapsed.inWholeHours}h"
        else -> "${elapsed.inWholeDays}d"
    }
}

fun projectName(cwd: String): String = cwd.trimEnd('/').substringAfterLast('/').ifEmpty { cwd }

fun isLaptop(os: String): Boolean = os.lowercase().let { it.contains("mac") || it.contains("darwin") || it.contains("windows") }
