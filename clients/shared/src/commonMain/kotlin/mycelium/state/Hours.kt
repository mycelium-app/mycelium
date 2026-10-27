package mycelium.state

import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant
import mycelium.Platform
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Group
import mycelium.api.Hours
import mycelium.api.WellnessSettings

const val WindDownMinutes = 15
const val OpeningMinutes = 15

private const val SearchDays = 8
private const val DayMinutes = 24 * 60

data class GroupHours(
    val group: Group,
    val open: Boolean,
    val openedUntil: Instant?,
    val closesAt: Instant?,
    val opensAt: Instant?,
    val closedAt: Instant?,
)

data class HoursView(
    val now: Instant,
    val local: String,
    val appOpen: Boolean,
    val appOpensAt: Instant?,
    val groups: Map<String, GroupHours>,
) {
    val closedGroupIds: Set<String> get() = groups.values.filterNot { it.open }.map { it.group.id }.toSet()

    fun closed(groupId: String?): Boolean = if (groupId == null) !appOpen else groups[groupId]?.open == false

    fun reopensAt(groupId: String?): Instant? = if (groupId == null) appOpensAt else groups[groupId]?.opensAt

    fun opening(groupId: String?): GroupHours? = groups[groupId]?.takeIf { it.openedUntil != null }

    fun windingDown(groupIds: Collection<String>): List<GroupHours> =
        groupIds.mapNotNull { groups[it] }.filter { it.open && it.openedUntil == null && it.closesAt != null && it.closesAt - now <= WindDownMinutes.minutes }

    fun nobodyOpened(): Boolean = !appOpen && groups.values.none { it.openedUntil != null }
}

fun localTimeOf(platform: Platform?, at: Instant): String = platform?.localDateTime(at.toEpochMilliseconds()) ?: at.toString().take(16)

private fun isOpenTogether(hours: List<Hours>, weekday: Int, minute: Int): Boolean = hours.all { isOpenAt(it, weekday, minute) }

private fun minuteOf(local: String): Int = local.substringAfter('T').take(5).let { time -> time.substringBefore(':').toInt() * 60 + time.substringAfter(':').toInt() }

private fun stateAt(hours: List<Hours>, weekday: Int, minute: Int, offset: Int): Boolean {
    val total = minute + offset
    val days = total.floorDiv(DayMinutes)

    return isOpenTogether(hours, (weekday + days).mod(7), total.mod(DayMinutes))
}

fun minutesUntilChange(hours: List<Hours>, local: String): Int? {
    if (hours.all { it.alwaysOpen }) {
        return null
    }

    val weekday = weekdayOf(local.substringBefore('T'))
    val minute = minuteOf(local)
    val before = stateAt(hours, weekday, minute, 0)

    return (1..SearchDays * DayMinutes).firstOrNull { stateAt(hours, weekday, minute, it) != before }
}

fun minutesSinceChange(hours: List<Hours>, local: String): Int? {
    if (hours.all { it.alwaysOpen }) {
        return null
    }

    val weekday = weekdayOf(local.substringBefore('T'))
    val minute = minuteOf(local)
    val now = stateAt(hours, weekday, minute, 0)
    val flipped = (1..SearchDays * DayMinutes).firstOrNull { stateAt(hours, weekday, minute, -it) != now }

    return flipped?.minus(1)
}

fun hoursViewOf(settings: WellnessSettings, groups: Collection<Group>, now: Instant, local: String): HoursView {
    val app = settings.hours
    val appOpen = isOpen(app, local)
    val appChange = minutesUntilChange(listOf(app), local)?.minutes?.let { now + it }
    val statuses = groups.associate { group ->
        val openedUntil = group.openedUntil?.let(::instantOrNull)?.takeIf { it > now }
        val together = listOf(app, group.hours)
        val byHours = isOpen(app, local) && isOpen(group.hours, local)
        val open = openedUntil != null || byHours
        val change = minutesUntilChange(together, local)?.minutes?.let { now + it }
        val last = minutesSinceChange(together, local)?.minutes?.let { now - it }

        group.id to GroupHours(
            group = group,
            open = open,
            openedUntil = openedUntil,
            closesAt = if (openedUntil != null) openedUntil else change.takeIf { byHours },
            opensAt = change.takeIf { !open },
            closedAt = last.takeIf { !open },
        )
    }

    return HoursView(now, local, appOpen, appChange.takeIf { !appOpen }, statuses)
}

fun hoursSummary(hours: Hours): String {
    if (hours.alwaysOpen) {
        return "Always open"
    }

    val days = hours.days.sortedBy { it.weekday }
    val openDays = days.filter { !it.closed }
    val times = openDays.map { it.opens to it.closes }.distinct()

    if (openDays.isEmpty()) {
        return "Closed all week"
    }

    if (times.size > 1) {
        return "Set hours, ${openDays.size} days"
    }

    val range = "${times.single().first} to ${times.single().second}"
    val weekdays = openDays.map { it.weekday }

    return when {
        weekdays == (0..6).toList() -> "Every day $range"
        weekdays == (0..4).toList() -> "Weekdays $range"
        else -> "${weekdays.joinToString(", ") { WeekdayNames[it].take(3) }} $range"
    }
}

fun clockOf(local: String): String = local.substringAfter('T').take(5)

fun dayWord(from: String, to: String): String? {
    val ahead = (weekdayOf(to.substringBefore('T')) - weekdayOf(from.substringBefore('T')) + 7) % 7

    return when (ahead) {
        0 -> null
        1 -> "tomorrow"
        else -> WeekdayNames[weekdayOf(to.substringBefore('T'))]
    }
}

fun displayName(group: String): String = group.replaceFirstChar { it.uppercase() }

fun namesJoined(names: List<String>): String = when (names.size) {
    0 -> ""
    1 -> names.single()
    else -> names.dropLast(1).joinToString(", ") + " and " + names.last()
}

enum class HoursAction { OpenHours, CloseNow }

data class HoursNotice(val message: String, val minutesLeft: Int, val action: HoursAction, val groupId: String)

data class ClosedNotice(val title: String, val body: String, val groupId: String?, val openNow: List<Group>, val openLabel: String)

private fun clockAt(platform: Platform?, at: Instant): String = clockOf(localTimeOf(platform, at))

private fun minutesLeft(now: Instant, until: Instant): Int = (((until - now).inWholeSeconds + 59) / 60).toInt().coerceAtLeast(1)

fun hoursNotice(view: HoursView, selected: String, platform: Platform?): HoursNotice? {
    val inView = if (selected == AllGroups) view.groups.values.map { it.group.id } else listOf(selected)
    val opened = inView.mapNotNull { view.opening(it) }.minByOrNull { it.openedUntil!! }

    if (opened != null) {
        val until = opened.openedUntil!!

        return HoursNotice("${displayName(opened.group.name)} is open out of hours until ${clockAt(platform, until)}.", minutesLeft(view.now, until), HoursAction.CloseNow, opened.group.id)
    }

    val closing = view.windingDown(inView).sortedBy { it.closesAt }
    val first = closing.firstOrNull() ?: return null
    val at = clockAt(platform, first.closesAt!!)
    val together = closing.filter { clockAt(platform, it.closesAt!!) == at }
    val names = together.map { displayName(it.group.name) }.sorted()
    val message = if (names.size == 1) "${names.single()} closes at $at. Anything you send now keeps running overnight." else "${namesJoined(names)} close at $at."

    return HoursNotice(message, minutesLeft(view.now, first.closesAt!!), HoursAction.OpenHours, first.group.id)
}

private fun reopens(view: HoursView, at: Instant?, platform: Platform?): String? {
    at ?: return null

    val local = localTimeOf(platform, at)
    val day = dayWord(view.local, local)

    return listOfNotNull(day, "at ${clockOf(local)}").joinToString(" ")
}

fun workingBehindClosedDoors(conversations: Collection<Conversation>, view: HoursView, selected: String): Int =
    conversations.count { it.archivedAt == null && it.status == ConversationStatus.Working && view.closed(it.groupId) && (selected == AllGroups || selected == NoGroup || it.groupId == selected) }

fun closedNotice(view: HoursView, selected: String, working: Int, platform: Platform?): ClosedNotice? {
    val chosen = view.groups[selected]
    val noneOpen = view.groups.values.none { it.open }
    val everythingClosed = when {
        chosen != null -> !chosen.open && !view.appOpen
        selected == NoGroup -> !view.appOpen
        else -> noneOpen && (!view.appOpen || view.groups.isNotEmpty())
    }

    if (everythingClosed) {
        val reopening = if (view.appOpen) view.groups.values.mapNotNull { it.opensAt }.minOrNull() else view.appOpensAt
        val until = reopens(view, reopening, platform)?.replace("at ", "")
        val title = if (until == null) "Mycelium is closed" else "Mycelium is closed until $until"
        val body = when (working) {
            0 -> "Results and notifications wait for the morning."
            1 -> "1 agent is still working. Its results and notifications wait for the morning."
            else -> "$working agents are still working. Their results and notifications wait for the morning."
        }

        return ClosedNotice(title, body, null, emptyList(), "Open a group for $OpeningMinutes minutes")
    }

    val closed = view.groups[selected]?.takeIf { !it.open } ?: return null
    val name = displayName(closed.group.name)
    val title = closed.closedAt?.let { "$name closed at ${clockAt(platform, it)}" } ?: "$name is closed"
    val opens = reopens(view, closed.opensAt, platform)?.let { "It opens again $it." } ?: "It has no hours to open this week."
    val afterwards = when (working) {
        0 -> null
        1 -> "1 agent is still working, and what it did will be here when it opens."
        else -> "$working agents are still working, and what they did will be here when it opens."
    }
    val open = view.groups.values.filter { it.open }.map { it.group }.sortedBy { it.name.lowercase() }

    return ClosedNotice(title, listOfNotNull(opens, afterwards).joinToString(" "), closed.group.id, open, "Open $name for $OpeningMinutes minutes")
}

fun sidebarClosedLine(view: HoursView, selected: String, platform: Platform?): String? {
    val notice = closedNotice(view, selected, 0, platform) ?: return null
    val until = notice.title.substringAfter("until ", "")

    return if (notice.groupId == null) "Everything is closed${if (until.isEmpty()) "." else " until $until."}" else {
        val closed = view.groups.getValue(notice.groupId)
        val reopening = reopens(view, closed.opensAt, platform)?.replace("at ", "")

        "${displayName(closed.group.name)} is closed${if (reopening == null) "." else " until $reopening."}"
    }
}

fun untilEarliestOpening(view: HoursView, platform: Platform?): String? =
    reopens(view, view.groups.values.filter { !it.open }.mapNotNull { it.opensAt }.minOrNull(), platform)?.replace("at ", "")

fun groupStatusText(view: HoursView, group: Group, platform: Platform?): String {
    val status = view.groups[group.id] ?: return hoursSummary(group.hours)

    if (status.openedUntil != null) {
        return "Open out of hours until ${clockAt(platform, status.openedUntil)}"
    }

    if (status.open) {
        return status.closesAt?.let { "Open now, closes at ${clockAt(platform, it)}" } ?: "Open now"
    }

    return reopens(view, status.opensAt, platform)?.let { "Closed, opens $it" } ?: "Closed"
}
