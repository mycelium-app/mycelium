package mycelium.state

import mycelium.api.Goal
import mycelium.api.GoalDelivery
import mycelium.api.GoalState
import kotlin.math.roundToInt
import kotlin.time.Clock
import kotlin.time.Instant

const val GoalCheckKind = "check"

data class GoalLimits(val tokenBudget: Long? = null, val timeLimitSeconds: Long? = null, val fiveHourPercent: Int? = null, val weeklyPercent: Int? = null)

fun goalProgress(goal: Goal, now: Instant = Clock.System.now()): String {
    val tokens = if (!goal.tokensKnown) "Token usage pending" else goal.tokenBudget?.let { "${formatTokens(goal.tokensUsed)} of ${formatTokens(it)} tokens" } ?: "${formatTokens(goal.tokensUsed)} tokens"
    val time = goal.timeLimitSeconds?.let { limit ->
        val started = goal.startedAt?.let { instantOrNull(it) }
        val held = goal.heldSeconds + (goal.heldSince?.let { instantOrNull(it) }?.let { (now - it).inWholeSeconds.coerceAtLeast(0) } ?: 0)
        val left = started?.let { limit - (now - it).inWholeSeconds + held }

        left?.let { if (it <= 0) "time limit reached" else "${formatTimeLeft(it)} left" }
    }
    val windows = listOfNotNull(
        goal.fiveHourLimit?.let { windowProgress("5-hour", goal.fiveHourUsed) },
        goal.weeklyLimit?.let { windowProgress("weekly", goal.weeklyUsed) },
    )

    return (listOf("Turn ${goal.turns + if (goal.turnActive) 1 else 0}", tokens) + listOfNotNull(time) + windows).joinToString("  ·  ")
}

private fun windowProgress(name: String, used: Double?): String = if (used == null) "$name usage unknown" else "$name ${used.roundToInt()}%"

fun formatTimeLeft(seconds: Long): String {
    val hours = seconds / 3600
    val minutes = seconds % 3600 / 60

    return when {
        hours > 0 && minutes > 0 -> "${hours}h ${minutes}m"
        hours > 0 -> "${hours}h"
        minutes > 0 -> "${minutes}m"
        else -> "under 1m"
    }
}

fun goalIsHeld(goal: Goal): Boolean = goal.state == GoalState.Active && goal.heldSince != null

fun goalMessage(goal: Goal, agent: String = ""): String? = when (goal.state) {
    GoalState.Active -> when {
        goal.heldSince != null -> "The goal carries on when the limit resets. Time on hold does not count towards the time limit."
        goal.delivery == GoalDelivery.Pending && goal.turnActive -> "Sends to ${agentName(agent)} when its current turn ends."
        goal.delivery == GoalDelivery.Pending -> "Sending the goal to ${agentName(agent)}."
        goal.delivery == GoalDelivery.Failed -> goal.deliveryReason
        else -> null
    }
    GoalState.Paused -> goal.reason ?: "Paused"
    GoalState.Met -> "Goal met. ${goal.reason.orEmpty()}".trim()
    GoalState.Stuck -> "Goal stuck. ${goal.reason.orEmpty()}".trim()
    GoalState.OverBudget -> goal.reason ?: "Over budget"
    GoalState.LimitReached -> goal.reason ?: "Limit reached"
}

fun parseTokenBudget(text: String): Long? {
    val cleaned = text.trim().lowercase().replace(",", "").replace(" ", "")
    val multiplier = when (cleaned.lastOrNull()) {
        'k' -> 1_000.0
        'm' -> 1_000_000.0
        'b' -> 1_000_000_000.0
        else -> 1.0
    }
    val number = (if (multiplier == 1.0) cleaned else cleaned.dropLast(1)).toDoubleOrNull() ?: return null
    val tokens = (number * multiplier).toLong()

    return tokens.takeIf { it > 0 }
}

fun parseTimeLimit(text: String): Long? {
    val cleaned = text.trim().lowercase().replace(" ", "")
    val parts = Regex("""(\d+(?:\.\d+)?)([hms])""").findAll(cleaned).toList()

    if (parts.isEmpty() || parts.joinToString("") { it.value } != cleaned) {
        return null
    }

    val seconds = parts.sumOf { part ->
        val unit = when (part.groupValues[2]) {
            "h" -> 3600.0
            "m" -> 60.0
            else -> 1.0
        }

        part.groupValues[1].toDouble() * unit
    }

    return seconds.toLong().takeIf { it > 0 }
}

fun parseWindowLimit(text: String): Int? = text.trim().removeSuffix("%").trim().toIntOrNull()?.takeIf { it in 1..100 }
