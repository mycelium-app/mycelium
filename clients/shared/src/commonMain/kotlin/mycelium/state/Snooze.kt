package mycelium.state

import kotlin.time.Duration
import kotlin.time.Duration.Companion.days
import kotlin.time.Duration.Companion.hours
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant

enum class SnoozeChoice(val label: String) {
    Hour("In 1 hour"),
    ThreeHours("In 3 hours"),
    Tomorrow("Tomorrow"),
    NextWeek("Next week"),
}

private val MorningHour = 8.hours
private const val DayMillis = 86_400_000L
private const val DaysFromThursdayToMonday = 3L

private fun localOffset(now: Instant, localDateTime: (Long) -> String?): Duration {
    val local = localDateTime(now.toEpochMilliseconds()) ?: return Duration.ZERO
    val wall = instantOrNull("$local:00Z") ?: return Duration.ZERO

    return (wall - now).inWholeMinutes.minutes
}

fun snoozeUntil(choice: SnoozeChoice, now: Instant, localDateTime: (Long) -> String?): Instant {
    val offset = localOffset(now, localDateTime)
    val wallMillis = (now + offset).toEpochMilliseconds()
    val epochDay = wallMillis.floorDiv(DayMillis)
    val daysUntilMonday = 7 - (epochDay + DaysFromThursdayToMonday).mod(7L)
    val morning = { days: Long -> Instant.fromEpochMilliseconds((epochDay + days) * DayMillis) + MorningHour - offset }

    return when (choice) {
        SnoozeChoice.Hour -> now + 1.hours
        SnoozeChoice.ThreeHours -> now + 3.hours
        SnoozeChoice.Tomorrow -> morning(1)
        SnoozeChoice.NextWeek -> morning(daysUntilMonday)
    }
}

fun customSnoozeUntil(text: String, now: Instant, localDateTime: (Long) -> String?): Instant? {
    val parts = text.trim().split(Regex("\\s+"))

    if (parts.size != 2) {
        return null
    }

    val wall = instantOrNull("${parts[0]}T${parts[1]}:00Z") ?: return null
    val moment = wall - localOffset(now, localDateTime)

    return moment.takeIf { it > now }
}

fun snoozeLeft(until: String, now: Instant): String {
    val left = (instantOrNull(until) ?: return "") - now

    return when {
        left >= 2.days -> "${left.inWholeDays}d left"
        left >= 1.hours -> "${left.inWholeHours}h left"
        else -> "${maxOf(left.inWholeMinutes, 1)}m left"
    }
}
