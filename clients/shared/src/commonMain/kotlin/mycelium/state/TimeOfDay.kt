package mycelium.state

import kotlin.time.Duration.Companion.days
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant
import mycelium.Platform
import mycelium.api.ActivityDay
import mycelium.api.ActivityHours
import mycelium.api.ActivitySpan

enum class ActivityKind(val wire: String, val legend: String, val noun: String) {
    Conversation("conversation", "Conversations", "Conversation"),
    Chat("chat", "Chats", "Chat"),
    Assistant("assistant", "Assistants", "Assistant"),
    Elsewhere("elsewhere", "Elsewhere in Mycelium", "Elsewhere in Mycelium"),
}

fun activityKindOf(wire: String): ActivityKind = ActivityKind.entries.firstOrNull { it.wire == wire } ?: ActivityKind.Elsewhere

data class ActivityTarget(
    val kind: ActivityKind,
    val route: String,
    val title: String? = null,
    val groupId: String? = null,
    val conversationId: String? = null,
    val assistantId: String? = null,
)

val ElsewhereTarget = ActivityTarget(ActivityKind.Elsewhere, "empty", "Mycelium")

val ActivityIdleLimit = 3.minutes

const val ActivityTableRows = 6

const val HeatLevels = 5

data class ActivityView(
    val shown: String? = null,
    val day: ActivityDay? = null,
    val hours: ActivityHours? = null,
    val loaded: Boolean = false,
    val failed: Boolean = false,
) {
    val today: String? get() = hours?.days?.lastOrNull()?.date

    val date: String? get() = shown ?: day?.date ?: today

    val onToday: Boolean get() = shown == null || shown == today
}

private const val MinutesInDay = 24 * 60

private val MonthNames = listOf("January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December")

fun addDays(isoDate: String, days: Int): String = (Instant.parse("${isoDate}T00:00:00Z") + days.days).toString().take(10)

fun dayHeading(isoDate: String): String {
    val (_, month, day) = isoDate.split('-')

    return "${WeekdayNames[weekdayOf(isoDate)]} ${day.toInt()} ${MonthNames[month.toInt() - 1]}"
}

fun dateWords(isoDate: String): String {
    val (_, month, day) = isoDate.split('-')

    return "${day.toInt()} ${MonthNames[month.toInt() - 1]}"
}

fun heatLevel(minutes: Long): Int = when {
    minutes <= 0 -> 0
    minutes < 15 -> 1
    minutes < 30 -> 2
    minutes < 45 -> 3
    else -> 4
}

fun heatName(isoDate: String, hour: Int, minutes: Long): String {
    val amount = when (minutes) {
        0L -> "nothing"
        1L -> "1 minute"
        else -> "$minutes minutes"
    }

    return "${WeekdayNames[weekdayOf(isoDate)]} ${clockHour(hour)} to ${clockHour((hour + 1) % 24)}, $amount"
}

fun clockHour(hour: Int): String = "${hour.toString().padStart(2, '0')}:00"

data class Stretch(val span: ActivitySpan, val kind: ActivityKind, val startMinute: Int, val endMinute: Int) {
    val minutes: Int get() = endMinute - startMinute
}

data class TimelineRange(val startHour: Int, val endHour: Int) {
    val startMinute: Int get() = startHour * 60

    val minutes: Int get() = (endHour - startHour) * 60

    fun fraction(minute: Int): Float = (minute - startMinute).toFloat() / minutes
}

fun minuteOfDay(local: String): Int = clockOf(local).let { clock -> clock.substringBefore(':').toInt() * 60 + clock.substringAfter(':').toInt() }

fun stretchOf(span: ActivitySpan, platform: Platform?): Stretch? {
    val start = instantOrNull(span.start) ?: return null
    val end = instantOrNull(span.end) ?: return null
    val startMinute = minuteOfDay(localTimeOf(platform, start))
    val endMinute = minOf(startMinute + ((end - start).inWholeSeconds / 60).toInt(), MinutesInDay)

    return Stretch(span, activityKindOf(span.kind), startMinute, endMinute).takeIf { it.minutes > 0 }
}

fun stretchesOf(spans: List<ActivitySpan>, platform: Platform?): List<Stretch> {
    var reached = 0

    return spans.mapNotNull { stretchOf(it, platform) }.sortedBy { it.startMinute }.mapNotNull { stretch ->
        val clipped = stretch.copy(startMinute = maxOf(stretch.startMinute, reached))

        reached = maxOf(reached, stretch.endMinute)
        clipped.takeIf { it.minutes > 0 }
    }
}

fun rangeOf(stretches: List<Stretch>): TimelineRange? {
    if (stretches.isEmpty()) {
        return null
    }

    val first = stretches.minOf { it.startMinute } / 60
    val last = (stretches.maxOf { it.endMinute } + 59) / 60

    return TimelineRange(first, maxOf(last, first + 1))
}

fun clockMinutes(minute: Int): String = "${(minute / 60 % 24).toString().padStart(2, '0')}:${(minute % 60).toString().padStart(2, '0')}"

data class ActivityTableRow(
    val kind: ActivityKind,
    val title: String,
    val project: String?,
    val group: String?,
    val minutes: Long,
    val conversationId: String?,
    val assistantId: String?,
)

private fun seconds(span: ActivitySpan): Long {
    val start = instantOrNull(span.start) ?: return 0
    val end = instantOrNull(span.end) ?: return 0

    return (end - start).inWholeSeconds
}

fun activityRows(spans: List<ActivitySpan>): List<ActivityTableRow> {
    val totals = LinkedHashMap<String, Pair<ActivitySpan, Long>>()
    val elsewhere = LinkedHashMap<String, Long>()

    for (span in spans) {
        if (activityKindOf(span.kind) == ActivityKind.Elsewhere) {
            elsewhere[span.title] = (elsewhere[span.title] ?: 0) + seconds(span)
        } else {
            val key = "${span.kind}:${span.conversationId ?: span.assistantId ?: span.title}"
            val before = totals[key]

            totals[key] = (before?.first ?: span) to ((before?.second ?: 0) + seconds(span))
        }
    }

    val rows = totals.values.map { (span, total) -> ActivityTableRow(activityKindOf(span.kind), span.title, span.project, span.group, (total + 30) / 60, span.conversationId, span.assistantId) }.toMutableList()

    if (elsewhere.isNotEmpty()) {
        rows += ActivityTableRow(ActivityKind.Elsewhere, elsewhere.keys.joinToString(", "), null, null, (elsewhere.values.sum() + 30) / 60, null, null)
    }

    return rows.sortedByDescending { it.minutes }.take(ActivityTableRows)
}
