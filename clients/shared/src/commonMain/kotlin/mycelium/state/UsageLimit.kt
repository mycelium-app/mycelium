package mycelium.state

import kotlin.time.Clock
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant

private val ResumeDelay = 1.minutes

fun limitName(window: String): String = when (window) {
    "five_hour" -> "5-hour limit"
    "seven_day" -> "weekly limit"
    "seven_day_opus" -> "weekly Opus limit"
    else -> "usage limit"
}

fun resumeTimeAfter(resetsAt: String): String? = runCatching { (Instant.parse(resetsAt) + ResumeDelay).toString() }.getOrNull()

fun resetBefore(resumeAt: String): String? = runCatching { (Instant.parse(resumeAt) - ResumeDelay).toString() }.getOrNull()

fun clockTime(iso: String, now: Instant = Clock.System.now(), localDateTime: (Long) -> String?): String {
    val moment = runCatching { Instant.parse(iso) }.getOrNull() ?: return ""
    val local = localDateTime(moment.toEpochMilliseconds()) ?: moment.toString().substringBefore('Z')
    val today = (localDateTime(now.toEpochMilliseconds()) ?: now.toString().substringBefore('Z')).substringBefore('T')
    val time = local.substringAfter('T').take(5)
    val (_, month, day) = local.substringBefore('T').split('-')

    return if (local.substringBefore('T') == today) time else "${day.toInt()} ${MonthNames[month.toInt() - 1]} $time"
}

private val MonthNames = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")
