package mycelium.state

import kotlin.time.Clock
import kotlin.time.Instant
import mycelium.api.Conversation

const val ChatAgent = "chat"

const val SubagentKind = "subagent"

const val TodayName = "Today"
const val ThisWeekName = "This week"
const val OlderName = "Older"

private const val SecondsPerDay = 86_400L
private const val DaysInWeek = 7

data class ChatSection(val name: String, val rows: List<Conversation>)

val Conversation.isChat: Boolean get() = agent == ChatAgent

val Conversation.inChatList: Boolean get() = isChat || scratch

val Conversation.isAssistantSession: Boolean get() = assistantId != null

val Conversation.isSubagent: Boolean get() = sideKind == SubagentKind

fun pinnedChats(conversations: Collection<Conversation>): List<Conversation> =
    conversations.filter { it.inChatList && !it.isAssistantSession && it.archivedAt == null && it.pinnedAt != null && it.parentId == null }.sortedBy { it.pinnedAt }

fun chatSections(conversations: Collection<Conversation>, now: Instant = Clock.System.now()): List<ChatSection> {
    val active = conversations.filter { it.inChatList && !it.isAssistantSession && it.archivedAt == null && it.pinnedAt == null }
    val activeIds = active.map { it.id }.toSet()
    val today = (now.epochSeconds).floorDiv(SecondsPerDay)

    val named = active
        .filter { it.parentId == null || it.parentId !in activeIds }
        .sortedByDescending { it.updatedAt }
        .groupBy { chat ->
            val daysAgo = instantOrNull(chat.updatedAt)?.let { today - it.epochSeconds.floorDiv(SecondsPerDay) }

            when {
                daysAgo == null || daysAgo >= DaysInWeek -> OlderName
                daysAgo >= 1 -> ThisWeekName
                else -> TodayName
            }
        }

    return listOf(TodayName, ThisWeekName, OlderName).mapNotNull { name -> named[name]?.let { ChatSection(name, it) } }
}
