package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class HoursDay(val weekday: Int, val closed: Boolean, val opens: String, val closes: String)

@Serializable
data class Hours(
    @SerialName("always_open") val alwaysOpen: Boolean,
    val days: List<HoursDay>,
)

val AlwaysOpenHours = Hours(alwaysOpen = true, days = (0..6).map { HoursDay(it, closed = it >= 5, opens = "09:00", closes = "18:00") })

@Serializable
data class OpenGroupRequest(val reason: String)

@Serializable
data class GroupOpening(@SerialName("opened_until") val openedUntil: String)

@Serializable
data class OutOfHoursOpen(
    @SerialName("group_id") val groupId: String,
    @SerialName("group_name") val groupName: String,
    val reason: String,
    @SerialName("opened_at") val openedAt: String,
    val until: String,
    @SerialName("closed_at") val closedAt: String? = null,
)

@Serializable
data class WellnessSettings(
    @SerialName("break_reminders") val breakReminders: Boolean,
    @SerialName("break_after_minutes") val breakAfterMinutes: Int,
    @SerialName("snooze_minutes") val snoozeMinutes: Int,
    @SerialName("break_minutes") val breakMinutes: Int,
    val hours: Hours,
    @SerialName("quiet_hold_notifications") val quietHoldNotifications: Boolean,
    @SerialName("quiet_hold_schedules") val quietHoldSchedules: Boolean,
    @SerialName("quiet_hold_pings") val quietHoldPings: Boolean,
    @SerialName("time_zone") val timeZone: String? = null,
)

@Serializable
data class WellnessHeartbeat(val seconds: Int)

@Serializable
data class StartBreakRequest(val minutes: Int? = null)

@Serializable
data class WellnessBreak(
    val id: String,
    @SerialName("started_at") val startedAt: String,
    @SerialName("ends_at") val endsAt: String,
    @SerialName("ended_early_at") val endedEarlyAt: String? = null,
)

@Serializable
data class WellnessDay(
    val day: String,
    @SerialName("viewing_minutes") val viewingMinutes: Long,
    @SerialName("agent_minutes") val agentMinutes: Long,
    val breaks: Long,
)

@Serializable
data class WellnessOverview(
    val days: List<WellnessDay>,
    @SerialName("last_break_at") val lastBreakAt: String? = null,
    @SerialName("current_break") val currentBreak: WellnessBreak? = null,
    @SerialName("quiet_now") val quietNow: Boolean = false,
    val held: Long = 0,
    @SerialName("held_week") val heldWeek: HeldWeek = HeldWeek(),
    @SerialName("opened_out_of_hours") val openedOutOfHours: List<OutOfHoursOpen> = emptyList(),
)

@Serializable
data class HeldWeek(val notifications: Long = 0, val schedules: Long = 0, val pings: Long = 0)

@Serializable
data class LearnedItem(
    val id: String,
    val text: String,
    @SerialName("source_kind") val sourceKind: String,
    @SerialName("conversation_id") val conversationId: String,
    @SerialName("conversation_title") val conversationTitle: String,
    @SerialName("message_id") val messageId: Long? = null,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class HoldRequest(val kind: String, val subject: String, @SerialName("group_id") val groupId: String? = null)

@Serializable
data class PuzzleResult(
    val solved: Boolean? = null,
    val score: Int? = null,
    val seconds: Int? = null,
    val source: String? = null,
)

@Serializable
data class PuzzleDay(
    val day: String,
    val result: PuzzleResult,
    @SerialName("recorded_at") val recordedAt: String,
)

@Serializable
data class PuzzleBucket(val value: Int, val count: Long)

@Serializable
data class PuzzleStats(
    val played: Long = 0,
    val solved: Long = 0,
    val average: Double? = null,
    @SerialName("average_seconds") val averageSeconds: Double? = null,
    val streak: Long = 0,
    @SerialName("best_streak") val bestStreak: Long = 0,
    val distribution: List<PuzzleBucket> = emptyList(),
)

@Serializable
data class PuzzleSummary(
    val puzzle: String,
    val day: String = "",
    val today: PuzzleResult? = null,
    val history: List<PuzzleDay> = emptyList(),
    val stats: PuzzleStats = PuzzleStats(),
)

@Serializable
data class WordleGuess(val word: String, val marks: List<String>)

@Serializable
data class WordleGame(
    val day: String,
    val status: String,
    val guesses: List<WordleGuess> = emptyList(),
    val answer: String? = null,
    val seconds: Int? = null,
)

@Serializable
data class WordleGuessRequest(val word: String)

@Serializable
data class ConnectionsGroup(val title: String, val words: List<String>, val level: Int)

@Serializable
data class ConnectionsGame(
    val day: String,
    val status: String,
    val words: List<String> = emptyList(),
    val solved: List<ConnectionsGroup> = emptyList(),
    val unsolved: List<ConnectionsGroup> = emptyList(),
    val mistakes: Int = 0,
    @SerialName("mistakes_allowed") val mistakesAllowed: Int = 4,
    val seconds: Int? = null,
)

@Serializable
data class ConnectionsGuessRequest(val words: List<String>)

@Serializable
data class ConnectionsGuessAnswer(val outcome: String, val game: ConnectionsGame)
