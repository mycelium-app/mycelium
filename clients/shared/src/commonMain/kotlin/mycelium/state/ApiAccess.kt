package mycelium.state

fun apiScopeLabel(scope: String): String = when (scope) {
    "conversations:read" -> "Read conversations"
    "conversations:write" -> "Send messages and change conversations"
    "nodes:read" -> "Read nodes"
    "settings:read" -> "Read settings"
    else -> scope
}

fun apiAccessDuration(hours: Int): String = if (hours == 1) "1 hour" else "$hours hours"
