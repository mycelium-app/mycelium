package mycelium.state

import mycelium.api.ChatToolServer

private val MentionKinds = setOf("stoat", "discord")

fun takesMentions(server: ChatToolServer): Boolean = server.kind in MentionKinds

fun mentionsHint(kind: String): String =
    if (kind == "discord") "When someone mentions the bot or messages it directly, the message goes to the assistant you choose." else "When someone mentions you or messages you directly, the message goes to the assistant you choose."

fun mentionWhen(server: ChatToolServer): String = if (server.kind == "discord") "Someone mentions the bot on Discord" else "Someone mentions you on Stoat"

fun mentionPlatform(server: ChatToolServer): String = if (server.kind == "discord") "Discord" else "Stoat"
