package mycelium.state

import mycelium.api.EachRun

val Weekdays = listOf("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")

enum class WhenKind(val label: String) {
    Every("Every"),
    Daily("Daily"),
    Weekly("Weekly"),
    Cron("Cron"),
}

private val WeekdayNumber = Weekdays.withIndex().associate { (index, name) -> name to index + 1 }

private fun time(minute: String, hour: String): String? {
    val minutes = minute.toIntOrNull()?.takeIf { it in 0..59 } ?: return null
    val hours = hour.toIntOrNull()?.takeIf { it in 0..23 } ?: return null

    return "${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}"
}

fun describeCron(cron: String): String {
    val fields = cron.trim().split(Regex("\\s+"))

    if (fields.size != 5) {
        return cron
    }

    val (minute, hour, dayOfMonth, month, dayOfWeek) = fields
    val calendar = dayOfMonth == "*" && month == "*"
    val every = minute.removePrefix("*/").takeIf { minute.startsWith("*/") }?.toIntOrNull()
    val everyHours = hour.removePrefix("*/").takeIf { hour.startsWith("*/") }?.toIntOrNull()

    return when {
        calendar && dayOfWeek == "*" && every != null && hour == "*" -> if (every == 1) "Every minute" else "Every $every minutes"
        calendar && dayOfWeek == "*" && minute == "0" && everyHours != null -> if (everyHours == 1) "Every hour" else "Every $everyHours hours"
        calendar && dayOfWeek == "*" -> time(minute, hour)?.let { "Daily $it" } ?: cron
        calendar && dayOfWeek == "1-5" -> time(minute, hour)?.let { "Weekdays $it" } ?: cron
        calendar -> {
            val day = dayOfWeek.toIntOrNull()?.let { number -> Weekdays.getOrNull((number + 6) % 7) }
            val at = time(minute, hour)

            if (day != null && at != null) "$day $at" else cron
        }
        else -> cron
    }
}

fun describeInWords(cron: String, timeZone: String): String {
    val plain = describeCron(cron)
    val sentence = Regex("^(\\w+) (\\d\\d:\\d\\d)$").matchEntire(plain)?.let { "${it.groupValues[1]} at ${it.groupValues[2]}" } ?: plain

    return "$sentence, $timeZone"
}

fun cronTime(cron: String): String? {
    val fields = cron.trim().split(Regex("\\s+"))

    return if (fields.size == 5) time(fields[0], fields[1]) else null
}

fun cronWithTime(cron: String, at: String): String? {
    val fields = cron.trim().split(Regex("\\s+"))
    val parts = at.trim().split(':')
    val hour = parts.getOrNull(0)?.toIntOrNull()?.takeIf { it in 0..23 }
    val minute = parts.getOrNull(1)?.toIntOrNull()?.takeIf { it in 0..59 }

    if (fields.size != 5 || parts.size != 2 || hour == null || minute == null) {
        return null
    }

    return (listOf(minute.toString(), hour.toString()) + fields.drop(2)).joinToString(" ")
}

fun cronFor(kind: WhenKind, every: Int, hours: Boolean, at: String, weekday: String, custom: String): String? {
    val parts = at.trim().split(':')
    val hour = parts.getOrNull(0)?.toIntOrNull()?.takeIf { it in 0..23 }
    val minute = parts.getOrNull(1)?.toIntOrNull()?.takeIf { it in 0..59 }
    val clock = if (parts.size == 2 && hour != null && minute != null) "$minute $hour" else null

    return when (kind) {
        WhenKind.Every -> if (every < 1) null else if (hours) "0 */$every * * *" else "*/$every * * * *"
        WhenKind.Daily -> clock?.let { "$it * * *" }
        WhenKind.Weekly -> clock?.let { "$it * * ${WeekdayNumber.getValue(weekday)}" }
        WhenKind.Cron -> custom.trim().takeIf { it.isNotEmpty() }
    }
}

data class Cadence(
    val kind: WhenKind = WhenKind.Weekly,
    val every: String = "1",
    val hours: Boolean = true,
    val at: String = "06:00",
    val weekday: String = Weekdays.first(),
    val custom: String = "",
)

fun cadenceOf(cron: String): Cadence {
    val fields = cron.trim().split(Regex("\\s+"))

    if (fields.size != 5) {
        return Cadence(WhenKind.Cron, custom = cron)
    }

    val (minute, hour, dayOfMonth, month, dayOfWeek) = fields
    val calendar = dayOfMonth == "*" && month == "*"
    val everyMinutes = minute.removePrefix("*/").takeIf { minute.startsWith("*/") }?.toIntOrNull()
    val everyHours = hour.removePrefix("*/").takeIf { hour.startsWith("*/") }?.toIntOrNull()
    val at = time(minute, hour)
    val day = dayOfWeek.toIntOrNull()?.let { number -> Weekdays.getOrNull((number + 6) % 7) }

    return when {
        calendar && dayOfWeek == "*" && everyMinutes != null && hour == "*" -> Cadence(WhenKind.Every, every = everyMinutes.toString(), hours = false)
        calendar && dayOfWeek == "*" && minute == "0" && everyHours != null -> Cadence(WhenKind.Every, every = everyHours.toString(), hours = true)
        calendar && dayOfWeek == "*" && at != null -> Cadence(WhenKind.Daily, at = at)
        calendar && day != null && at != null -> Cadence(WhenKind.Weekly, at = at, weekday = day)
        else -> Cadence(WhenKind.Cron, custom = cron)
    }
}

fun eachRunName(eachRun: String): String = if (eachRun == EachRun.Same) "Same conversation" else "New conversation"
