package mycelium.state

fun apiScopeLabel(scope: String): String = when (scope) {
    "conversations:read" -> "Read conversations"
    "conversations:write" -> "Send messages and change conversations"
    "nodes:read" -> "Read nodes"
    "settings:read" -> "Read settings"
    "assistants" -> "Allow managing assistants"
    else -> scope
}

fun apiAccessDuration(hours: Int): String = if (hours == 1) "1 hour" else "$hours hours"

fun sshAccessDuration(minutes: Int): String = when {
    minutes % 60 == 0 -> apiAccessDuration(minutes / 60)
    minutes < 60 -> "$minutes minutes"
    else -> "${apiAccessDuration(minutes / 60)} ${minutes % 60} minutes"
}
