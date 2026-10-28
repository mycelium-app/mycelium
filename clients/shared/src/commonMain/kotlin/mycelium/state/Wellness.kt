package mycelium.state

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlin.time.Clock
import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.api.PuzzleResult
import mycelium.api.PuzzleSummary
import mycelium.api.AlwaysOpenHours
import mycelium.api.Hours
import mycelium.api.WellnessBreak
import mycelium.api.WellnessOverview
import mycelium.api.WellnessHeartbeat
import mycelium.api.WellnessSettings


val DefaultWellnessSettings = WellnessSettings(
    breakReminders = true,
    breakAfterMinutes = 50,
    snoozeMinutes = 10,
    breakMinutes = 5,
    hours = AlwaysOpenHours,
    quietHoldNotifications = true,
    quietHoldSchedules = true,
    quietHoldPings = true,
)

val WeekdayNames = listOf("Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday")

private fun minutesOf(time: String): Int? {
    val (hours, minutes) = time.split(':').takeIf { it.size == 2 }?.map { it.toIntOrNull() ?: return null } ?: return null

    return hours * 60 + minutes
}

fun weekdayOf(isoDate: String): Int {
    val (year, month, day) = isoDate.split('-').map { it.toInt() }
    val offsets = intArrayOf(0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4)
    val shifted = if (month < 3) year - 1 else year
    val sundayFirst = (shifted + shifted / 4 - shifted / 100 + shifted / 400 + offsets[month - 1] + day) % 7

    return (sundayFirst + 6) % 7
}

fun isOpen(hours: Hours, localDateTime: String): Boolean {
    if (hours.alwaysOpen) {
        return true
    }

    val weekday = weekdayOf(localDateTime.substringBefore('T'))
    val now = minutesOf(localDateTime.substringAfter('T').take(5)) ?: return false

    return isOpenAt(hours, weekday, now)
}

internal fun isOpenAt(hours: Hours, weekday: Int, minute: Int): Boolean {
    if (hours.alwaysOpen) {
        return true
    }

    val yesterday = (weekday + 6) % 7
    val inToday = hours.days.filter { !it.closed && it.weekday == weekday }.any { day ->
        val opens = minutesOf(day.opens) ?: return@any false
        val closes = minutesOf(day.closes) ?: return@any false

        if (closes > opens) minute in opens until closes else minute >= opens
    }
    val overnight = hours.days.filter { !it.closed && it.weekday == yesterday }.any { day ->
        val opens = minutesOf(day.opens) ?: return@any false
        val closes = minutesOf(day.closes) ?: return@any false

        closes <= opens && minute < closes
    }

    return inToday || overnight
}

fun BreakPhase.holdsEverything(): Boolean = this == BreakPhase.OnBreak || this == BreakPhase.Noting

sealed interface BreakPhase {
    data object Idle : BreakPhase

    data object Due : BreakPhase

    data class Snoozed(val minutesLeft: Int) : BreakPhase

    data object OnBreak : BreakPhase

    data object Noting : BreakPhase
}

data class BreakClock(val phase: BreakPhase = BreakPhase.Idle, val activeMinutes: Int = 0, val blocksDone: Int = 0) {
    fun remindsAfter(settings: WellnessSettings): Int? = when {
        settings.breakReminders -> settings.breakAfterMinutes
        else -> null
    }

    fun tick(settings: WellnessSettings, quiet: Boolean): BreakClock {
        val threshold = remindsAfter(settings)

        if (threshold == null || quiet) {
            return this
        }

        return when (phase) {
            BreakPhase.Idle -> {
                val minutes = activeMinutes + 1

                if (minutes >= threshold) copy(phase = BreakPhase.Due, activeMinutes = minutes) else copy(activeMinutes = minutes)
            }

            is BreakPhase.Snoozed -> if (phase.minutesLeft <= 1) copy(phase = BreakPhase.Due) else copy(phase = BreakPhase.Snoozed(phase.minutesLeft - 1))
            BreakPhase.Due, BreakPhase.OnBreak, BreakPhase.Noting -> this
        }
    }

    fun snooze(settings: WellnessSettings): BreakClock = if (phase == BreakPhase.Due) copy(phase = BreakPhase.Snoozed(settings.snoozeMinutes)) else this

    fun dismiss(): BreakClock = copy(phase = BreakPhase.Idle, activeMinutes = 0)

    fun breakLength(settings: WellnessSettings): Int = settings.breakMinutes

    fun startBreak(): BreakClock = copy(phase = BreakPhase.OnBreak, activeMinutes = 0, blocksDone = blocksDone + 1)

    fun endBreak(): BreakClock = copy(phase = BreakPhase.Noting, activeMinutes = 0)

    fun backToWork(): BreakClock = copy(phase = BreakPhase.Idle, activeMinutes = 0)
}

fun countdownText(left: Duration): String {
    val seconds = left.inWholeSeconds.coerceAtLeast(0)

    return "${seconds / 60}:${(seconds % 60).toString().padStart(2, '0')}"
}

fun minutesLeftText(left: Duration): String {
    val minutes = ((left.inWholeSeconds + 59) / 60).coerceAtLeast(1)

    return "$minutes ${if (minutes == 1L) "minute" else "minutes"}"
}

fun stillWorkingOnBreak(working: Int): String = when (working) {
    0 -> "Results and notifications wait until the break ends."
    1 -> "1 agent is still working. Its results and notifications wait until the break ends."
    else -> "$working agents are still working. Their results and notifications wait until the break ends."
}

const val BreakNoteMissing = "Enter what you did in your break"

fun breakEndedCaption(endedAt: String, earlyMinutes: Int?, finished: Int): String {
    if (earlyMinutes != null) {
        return "You ended your break $earlyMinutes ${if (earlyMinutes == 1) "minute" else "minutes"} early."
    }

    val ended = "Your break ended at $endedAt."

    return when (finished) {
        0 -> ended
        1 -> "$ended 1 agent finished while you were away and its results are waiting."
        else -> "$ended $finished agents finished while you were away and their results are waiting."
    }
}

fun breakWhen(local: String, today: String): String {
    val date = local.substringBefore('T')
    val clock = clockOf(local)

    return when (date) {
        today -> "Today, $clock"
        addDays(today, -1) -> "Yesterday, $clock"
        else -> "${dayLabel(date)}, $clock"
    }
}

fun WellnessBreak.lengthText(): String {
    val ran = startedAt.let(::instantOrNull)?.let { start -> endedAt?.let(::instantOrNull)?.minus(start) }
    val minutes = ran?.let { ((it.inWholeSeconds + 30) / 60).toInt() }?.coerceAtLeast(1) ?: plannedMinutes

    return if (endedEarly) "$minutes min, ended early" else "$minutes min"
}

data class BreakEnd(val endedAt: Instant, val earlyMinutes: Int? = null)

data class HeldNotification(val title: String, val conversationId: String, val groupId: String? = null)

data class WellnessState(
    val settings: WellnessSettings = DefaultWellnessSettings,
    val overview: WellnessOverview? = null,
    val clock: BreakClock = BreakClock(),
    val breakStartedAt: Instant? = null,
    val breakEndsAt: Instant? = null,
    val breakId: String? = null,
    val ended: BreakEnd? = null,
    val held: List<HeldNotification> = emptyList(),
    val puzzles: List<PuzzleSummary> = emptyList(),
    val unreadable: Set<String> = emptySet(),
    val activity: ActivityView = ActivityView(),
    val loaded: Boolean = false,
)

class WellnessStore(private val api: MyceliumApi, private val scope: CoroutineScope, private val platform: Platform?) {
    private val mutableState = MutableStateFlow(WellnessState())
    private var lastInputAt = Clock.System.now()

    val state: StateFlow<WellnessState> = mutableState.asStateFlow()

    var presence: ActivityTarget = ElsewhereTarget

    suspend fun load() {
        val settings = catching { api.wellnessSettings() }.getOrNull() ?: return
        val overview = catching { api.wellnessOverview() }.getOrNull()
        val current = overview?.currentBreak
        val awaiting = overview?.awaitingNote
        val previous = mutableState.value.clock.phase
        val phase = when {
            current != null -> BreakPhase.OnBreak
            awaiting != null -> BreakPhase.Noting
            overview == null && previous == BreakPhase.Noting -> BreakPhase.Noting
            previous == BreakPhase.OnBreak || previous == BreakPhase.Noting -> BreakPhase.Idle
            else -> previous
        }

        val startedAt = current?.startedAt?.let(::instantOrNull)
        val endsAt = current?.endsAt?.let(::instantOrNull)
        val endedAt = awaiting?.endedAt?.let(::instantOrNull) ?: Clock.System.now()
        val ranFor = awaiting?.startedAt?.let(::instantOrNull)?.let { endedAt - it }
        val earlyMinutes = if (awaiting?.endedEarly == true && ranFor != null) (awaiting.plannedMinutes - ranFor.inWholeMinutes.toInt()).coerceAtLeast(1) else null

        mutableState.update {
            it.copy(
                settings = settings,
                overview = overview ?: it.overview,
                clock = it.clock.copy(phase = phase),
                breakStartedAt = if (phase == BreakPhase.OnBreak) startedAt ?: it.breakStartedAt else null,
                breakEndsAt = if (phase == BreakPhase.OnBreak) endsAt ?: it.breakEndsAt else null,
                breakId = current?.id ?: awaiting?.id ?: it.breakId.takeIf { phase != BreakPhase.Idle },
                ended = if (awaiting != null) BreakEnd(endedAt, earlyMinutes) else it.ended.takeIf { phase == BreakPhase.Noting },
                loaded = true,
            )
        }

        if (mutableState.value.activity.loaded) {
            loadActivity()
        }
    }

    fun reload() {
        scope.launch { load() }
    }

    fun closedNow(): Boolean {
        val local = platform?.localDateTime(Clock.System.now().toEpochMilliseconds()) ?: return false

        return !isOpen(mutableState.value.settings.hours, local)
    }

    fun holdsNotifications(closed: Boolean): Boolean {
        val current = mutableState.value

        return current.clock.phase.holdsEverything() || (current.settings.quietHoldNotifications && closed)
    }

    fun holdsPings(closed: Boolean): Boolean {
        val current = mutableState.value

        return current.clock.phase.holdsEverything() || (current.settings.quietHoldPings && closed)
    }

    fun hold(notification: HeldNotification, ping: Boolean = false) {
        mutableState.update { it.copy(held = it.held + notification) }
        scope.launch { catching { api.holdQuietly(if (ping) "ping" else "notification", notification.conversationId, notification.groupId) } }
    }

    fun releaseHeld(closed: (String?) -> Boolean): List<HeldNotification> {
        if (mutableState.value.clock.phase.holdsEverything()) {
            return emptyList()
        }

        val released = mutableState.value.held.filterNot { closed(it.groupId) }

        mutableState.update { it.copy(held = it.held - released.toSet()) }

        return released
    }

    fun tickActiveMinute() {
        val closed = closedNow()

        mutableState.update { it.copy(clock = it.clock.tick(it.settings, closed)) }
    }

    fun noteInput() {
        lastInputAt = Clock.System.now()
    }

    fun personActive(now: Instant): Boolean = now - lastInputAt <= ActivityIdleLimit

    fun heartbeat(seconds: Int, now: Instant) {
        val target = presence
        val beat = WellnessHeartbeat(seconds, now.toString(), target.kind.wire, target.route, target.groupId, target.conversationId, target.assistantId, target.title)

        scope.launch { catching { api.wellnessHeartbeat(beat) } }
    }

    suspend fun loadActivity() {
        val shown = mutableState.value.activity.shown
        val hours = catching { api.activityHours() }.getOrNull()
        val day = catching { api.activityDay(shown) }.getOrNull()

        mutableState.update {
            val activity = it.activity

            it.copy(activity = activity.copy(hours = hours ?: activity.hours, day = if (shown == activity.shown) day ?: activity.day else activity.day, loaded = true, failed = hours == null && day == null))
        }
    }

    fun showActivityDay(date: String) {
        val today = mutableState.value.activity.today
        val shown = date.takeUnless { it == today }

        mutableState.update { it.copy(activity = it.activity.copy(shown = shown, day = null, failed = false)) }
        scope.launch { loadActivity() }
    }

    fun stepActivityDay(days: Int) {
        val date = mutableState.value.activity.date ?: return

        showActivityDay(addDays(date, days))
    }

    fun update(change: (WellnessSettings) -> WellnessSettings) {
        val before = mutableState.value.settings
        val changed = change(before)

        mutableState.update { it.copy(settings = changed) }
        scope.launch {
            val saved = catching { api.saveWellnessSettings(changed) }.getOrNull()

            mutableState.update { it.copy(settings = saved ?: before) }
        }
    }

    fun snooze() {
        mutableState.update { it.copy(clock = it.clock.snooze(it.settings)) }
    }

    fun dismiss() {
        mutableState.update { it.copy(clock = it.clock.dismiss()) }
    }

    fun startBreak() {
        val minutes = mutableState.value.let { it.clock.breakLength(it.settings) }
        val startedAt = Clock.System.now()

        mutableState.update { it.copy(clock = it.clock.startBreak(), breakStartedAt = startedAt, breakEndsAt = startedAt + minutes.minutes) }
        scope.launch {
            catching { api.startWellnessBreak(minutes) }
            load()
        }
    }

    fun endBreak() {
        val now = Clock.System.now()
        val left = mutableState.value.breakEndsAt?.let { it - now } ?: Duration.ZERO
        val earlyMinutes = ((left.inWholeSeconds + 59) / 60).toInt().coerceAtLeast(1)

        mutableState.update { it.copy(clock = it.clock.endBreak(), breakStartedAt = null, breakEndsAt = null, ended = BreakEnd(now, earlyMinutes)) }
        scope.launch {
            catching { api.endWellnessBreak() }
            load()
        }
    }

    fun finishBreakIfOver(now: Instant) {
        val current = mutableState.value
        val endsAt = current.breakEndsAt ?: return

        if (current.clock.phase != BreakPhase.OnBreak || now < endsAt) {
            return
        }

        mutableState.update { it.copy(clock = it.clock.endBreak(), breakStartedAt = null, breakEndsAt = null, ended = BreakEnd(endsAt)) }
        reload()
    }

    suspend fun saveBreakNote(note: String) {
        if (mutableState.value.breakId == null) {
            load()
        }

        val id = mutableState.value.breakId ?: return

        api.saveBreakNote(id, note)
        mutableState.update { it.copy(clock = it.clock.backToWork(), breakId = null, ended = null) }
        load()
    }

    suspend fun loadPuzzles() {
        val puzzles = catching { api.puzzles() }.getOrNull() ?: return

        mutableState.update { it.copy(puzzles = puzzles) }
    }

    fun markUnreadable(puzzle: String) {
        mutableState.update { it.copy(unreadable = it.unreadable + puzzle) }
    }

    fun markReadable(puzzle: String) {
        mutableState.update { it.copy(unreadable = it.unreadable - puzzle) }
    }

    suspend fun recordManualResult(puzzle: String, result: PuzzleResult) {
        api.recordPuzzleResult(puzzle, result.copy(source = "manual"))
        loadPuzzles()
    }

    fun recordResult(puzzle: String, result: PuzzleResult) {
        scope.launch {
            catching { api.recordPuzzleResult(puzzle, result.copy(source = "read")) }
            loadPuzzles()
        }
    }
}
