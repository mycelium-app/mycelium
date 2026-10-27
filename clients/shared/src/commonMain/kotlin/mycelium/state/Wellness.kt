package mycelium.state

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlin.time.Clock
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.api.PuzzleResult
import mycelium.api.PuzzleSummary
import mycelium.api.AlwaysOpenHours
import mycelium.api.Hours
import mycelium.api.WellnessOverview
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

sealed interface BreakPhase {
    data object Idle : BreakPhase

    data object Due : BreakPhase

    data class Snoozed(val minutesLeft: Int) : BreakPhase

    data object OnBreak : BreakPhase
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
            BreakPhase.Due, BreakPhase.OnBreak -> this
        }
    }

    fun snooze(settings: WellnessSettings): BreakClock = if (phase == BreakPhase.Due) copy(phase = BreakPhase.Snoozed(settings.snoozeMinutes)) else this

    fun dismiss(): BreakClock = copy(phase = BreakPhase.Idle, activeMinutes = 0)

    fun breakLength(settings: WellnessSettings): Int = settings.breakMinutes

    fun startBreak(): BreakClock = copy(phase = BreakPhase.OnBreak, activeMinutes = 0, blocksDone = blocksDone + 1)

    fun endBreak(): BreakClock = copy(phase = BreakPhase.Idle, activeMinutes = 0)
}

data class HeldNotification(val title: String, val conversationId: String, val groupId: String? = null)

data class WellnessState(
    val settings: WellnessSettings = DefaultWellnessSettings,
    val overview: WellnessOverview? = null,
    val clock: BreakClock = BreakClock(),
    val held: List<HeldNotification> = emptyList(),
    val puzzles: List<PuzzleSummary> = emptyList(),
    val unreadable: Set<String> = emptySet(),
    val loaded: Boolean = false,
)

class WellnessStore(private val api: MyceliumApi, private val scope: CoroutineScope, private val platform: Platform?) {
    private val mutableState = MutableStateFlow(WellnessState())

    val state: StateFlow<WellnessState> = mutableState.asStateFlow()

    suspend fun load() {
        val settings = catching { api.wellnessSettings() }.getOrNull() ?: return
        val overview = catching { api.wellnessOverview() }.getOrNull()
        val current = overview?.currentBreak
        val phase = if (current != null) BreakPhase.OnBreak else mutableState.value.clock.phase.takeUnless { it == BreakPhase.OnBreak } ?: BreakPhase.Idle

        mutableState.update { it.copy(settings = settings, overview = overview ?: it.overview, clock = it.clock.copy(phase = phase), loaded = true) }
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

        return current.clock.phase == BreakPhase.OnBreak || (current.settings.quietHoldNotifications && closed)
    }

    fun holdsPings(closed: Boolean): Boolean {
        val current = mutableState.value

        return current.clock.phase == BreakPhase.OnBreak || (current.settings.quietHoldPings && closed)
    }

    fun hold(notification: HeldNotification, ping: Boolean = false) {
        mutableState.update { it.copy(held = it.held + notification) }
        scope.launch { catching { api.holdQuietly(if (ping) "ping" else "notification", notification.conversationId, notification.groupId) } }
    }

    fun releaseHeld(closed: (String?) -> Boolean): List<HeldNotification> {
        if (mutableState.value.clock.phase == BreakPhase.OnBreak) {
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

    fun heartbeat(seconds: Int) {
        scope.launch { catching { api.wellnessHeartbeat(seconds) } }
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

        mutableState.update { it.copy(clock = it.clock.startBreak()) }
        scope.launch {
            catching { api.startWellnessBreak(minutes) }
            load()
        }
    }

    fun endBreak() {
        mutableState.update { it.copy(clock = it.clock.endBreak()) }
        scope.launch {
            catching { api.endWellnessBreak() }
            load()
        }
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

    fun setPlayed(puzzle: String, played: Boolean) {
        scope.launch {
            catching { api.setPuzzlePlayed(puzzle, played) }
            loadPuzzles()
        }
    }

    fun recordResult(puzzle: String, result: PuzzleResult) {
        scope.launch {
            catching { api.recordPuzzleResult(puzzle, result.copy(source = "read")) }
            loadPuzzles()
        }
    }
}
