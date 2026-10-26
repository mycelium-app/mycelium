package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class QuietDay(
    val weekday: Int,
    val enabled: Boolean,
    @SerialName("all_day") val allDay: Boolean,
    val starts: String,
    val ends: String,
)

@Serializable
data class WellnessSettings(
    @SerialName("break_reminders") val breakReminders: Boolean,
    @SerialName("break_after_minutes") val breakAfterMinutes: Int,
    @SerialName("snooze_minutes") val snoozeMinutes: Int,
    @SerialName("break_minutes") val breakMinutes: Int,
    val pomodoro: Boolean,
    @SerialName("quiet_hours") val quietHours: Boolean,
    @SerialName("quiet_hold_notifications") val quietHoldNotifications: Boolean,
    @SerialName("quiet_hold_schedules") val quietHoldSchedules: Boolean,
    @SerialName("quiet_hold_pings") val quietHoldPings: Boolean,
    @SerialName("time_zone") val timeZone: String? = null,
    @SerialName("quiet_days") val quietDays: List<QuietDay>,
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
    val puzzles: List<String>,
)

@Serializable
data class WellnessOverview(
    val days: List<WellnessDay>,
    @SerialName("last_break_at") val lastBreakAt: String? = null,
    @SerialName("current_break") val currentBreak: WellnessBreak? = null,
    @SerialName("quiet_now") val quietNow: Boolean = false,
    val held: Long = 0,
)
