package mycelium.state

private val NodeIdLine = Regex("""^\s*node_id\s*=\s*"([0-9a-fA-F-]{36})"\s*$""", RegexOption.MULTILINE)

fun nodeIdFromConfig(toml: String): String? = NodeIdLine.find(toml)?.groupValues?.get(1)?.lowercase()?.takeUnless { it == NilNodeId }

private const val NilNodeId = "00000000-0000-0000-0000-000000000000"
