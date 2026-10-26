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
import mycelium.api.QuietDay
import mycelium.api.WellnessOverview
import mycelium.api.WellnessSettings

const val PomodoroFocusMinutes = 25
const val PomodoroBreakMinutes = 5
const val PomodoroLongBreakMinutes = 15
const val PomodoroBlocksBeforeLongBreak = 4

val DefaultQuietDays = (0..6).map { QuietDay(it, enabled = true, allDay = it >= 5, starts = "22:00", ends = "08:00") }

val DefaultWellnessSettings = WellnessSettings(
    breakReminders = true,
    breakAfterMinutes = 50,
    snoozeMinutes = 10,
    breakMinutes = 5,
    pomodoro = false,
    quietHours = false,
    quietHoldNotifications = true,
    quietHoldSchedules = true,
    quietHoldPings = true,
    quietDays = DefaultQuietDays,
)

val WeekdayNames = listOf("Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday")

val Puzzles = listOf(
    Puzzle("wordle", "Wordle", "https://www.nytimes.com/games/wordle/index.html"),
    Puzzle("connections", "Connections", "https://www.nytimes.com/games/connections"),
    Puzzle("daily-orbs", "Daily Orbs", "https://dailyorbs.com"),
    Puzzle("catfishing", "Catfishing", "https://catfishing.net"),
)

data class Puzzle(val id: String, val name: String, val url: String)

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

fun quietAt(settings: WellnessSettings, localDateTime: String): Boolean {
    if (!settings.quietHours) {
        return false
    }

    val today = weekdayOf(localDateTime.substringBefore('T'))
    val yesterday = (today + 6) % 7
    val now = minutesOf(localDateTime.substringAfter('T').take(5)) ?: return false

    val inToday = settings.quietDays.filter { it.enabled && it.weekday == today }.any { day ->
        val starts = minutesOf(day.starts) ?: return@any false
        val ends = minutesOf(day.ends) ?: return@any false

        day.allDay || if (ends > starts) now in starts until ends else now >= starts
    }
    val overnight = settings.quietDays.filter { it.enabled && !it.allDay && it.weekday == yesterday }.any { day ->
        val starts = minutesOf(day.starts) ?: return@any false
        val ends = minutesOf(day.ends) ?: return@any false

        ends <= starts && now < ends
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
        settings.pomodoro -> PomodoroFocusMinutes
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

    fun breakLength(settings: WellnessSettings): Int = when {
        !settings.pomodoro -> settings.breakMinutes
        (blocksDone + 1) % PomodoroBlocksBeforeLongBreak == 0 -> PomodoroLongBreakMinutes
        else -> PomodoroBreakMinutes
    }

    fun startBreak(): BreakClock = copy(phase = BreakPhase.OnBreak, activeMinutes = 0, blocksDone = blocksDone + 1)

    fun endBreak(): BreakClock = copy(phase = BreakPhase.Idle, activeMinutes = 0)
}

data class HeldNotification(val title: String, val conversationId: String)

data class WellnessState(
    val settings: WellnessSettings = DefaultWellnessSettings,
    val overview: WellnessOverview? = null,
    val clock: BreakClock = BreakClock(),
    val held: List<HeldNotification> = emptyList(),
    val loaded: Boolean = false,
)

class WellnessStore(private val api: MyceliumApi, private val scope: CoroutineScope, private val platform: Platform?) {
    private val mutableState = MutableStateFlow(WellnessState())

    val state: StateFlow<WellnessState> = mutableState.asStateFlow()

    suspend fun load() {
        val settings = runCatching { api.wellnessSettings() }.getOrNull() ?: return
        val overview = runCatching { api.wellnessOverview() }.getOrNull()
        val current = overview?.currentBreak
        val phase = if (current != null) BreakPhase.OnBreak else mutableState.value.clock.phase.takeUnless { it == BreakPhase.OnBreak } ?: BreakPhase.Idle

        mutableState.update { it.copy(settings = settings, overview = overview ?: it.overview, clock = it.clock.copy(phase = phase), loaded = true) }
    }

    fun reload() {
        scope.launch { load() }
    }

    fun quietNow(): Boolean {
        val local = platform?.localDateTime(Clock.System.now().toEpochMilliseconds()) ?: return false

        return quietAt(mutableState.value.settings, local)
    }

    fun holdsNotifications(): Boolean {
        val current = mutableState.value

        return current.clock.phase == BreakPhase.OnBreak || (current.settings.quietHoldNotifications && quietNow())
    }

    fun hold(notification: HeldNotification) {
        mutableState.update { it.copy(held = it.held + notification) }
    }

    fun releaseHeld(): List<HeldNotification> {
        if (mutableState.value.held.isEmpty() || holdsNotifications()) {
            return emptyList()
        }

        val released = mutableState.value.held

        mutableState.update { it.copy(held = emptyList()) }

        return released
    }

    fun tickActiveMinute() {
        val quiet = quietNow()

        mutableState.update { it.copy(clock = it.clock.tick(it.settings, quiet)) }
    }

    fun heartbeat(seconds: Int) {
        scope.launch { runCatching { api.wellnessHeartbeat(seconds) } }
    }

    fun update(change: (WellnessSettings) -> WellnessSettings) {
        val before = mutableState.value.settings
        val changed = change(before)

        mutableState.update { it.copy(settings = changed) }
        scope.launch {
            val saved = runCatching { api.saveWellnessSettings(changed) }.getOrNull()

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
            runCatching { api.startWellnessBreak(minutes) }
            load()
        }
    }

    fun endBreak() {
        mutableState.update { it.copy(clock = it.clock.endBreak()) }
        scope.launch {
            runCatching { api.endWellnessBreak() }
            load()
        }
    }

    fun setPlayed(puzzle: String, played: Boolean) {
        scope.launch {
            runCatching { api.setPuzzlePlayed(puzzle, played) }
            load()
        }
    }
}
