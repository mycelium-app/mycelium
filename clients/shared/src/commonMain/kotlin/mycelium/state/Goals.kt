package mycelium.state

import mycelium.api.Goal
import mycelium.api.GoalDelivery
import mycelium.api.GoalState
import mycelium.api.GoalWait
import kotlin.math.roundToInt
import kotlin.time.Clock
import kotlin.time.Instant

const val GoalCheckKind = "check"

data class GoalLimits(val tokenBudget: Long? = null, val timeLimitSeconds: Long? = null, val fiveHourPercent: Int? = null, val weeklyPercent: Int? = null, val repeatLimit: Int? = null)

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

fun goalIsWaiting(goal: Goal): Boolean = goal.state == GoalState.Active && goal.waitReason != null

fun goalCanNudgeNow(goal: Goal): Boolean = goal.state == GoalState.Active && (goal.waitReason == GoalWait.RepeatedOutcome || goal.waitReason == GoalWait.BackgroundWork)

fun goalIsBlocked(goal: Goal): Boolean = goal.state == GoalState.Stuck

fun goalMessage(goal: Goal, agent: String = "", at: (String) -> String = { it }): String? = when (goal.state) {
    GoalState.Active -> when {
        goal.waitReason == GoalWait.QuestionWaiting -> "Held for your answer to ${agentName(agent)}'s question. Mycelium nudges once you answer. Time on hold does not count towards the time limit."
        goal.heldSince != null -> "The goal carries on when the limit resets. Time on hold does not count towards the time limit."
        goal.waitReason == GoalWait.RepeatedOutcome -> "The last ${turnCount(goal.repeats)} turns ended the same way, so Mycelium is waiting before it nudges again${goal.nextNudgeAt?.let { " at ${at(it)}" }.orEmpty()}. If the next turn ends the same way too, the goal stops."
        goal.waitReason == GoalWait.BackgroundWork -> "${agentName(agent)} is waiting for background work. Mycelium nudges when it reports back${goal.nextNudgeAt?.let { ", or at ${at(it)} if it has not" }.orEmpty()}."
        goal.delivery == GoalDelivery.Pending && goal.turnActive -> "Sends to ${agentName(agent)} when its current turn ends."
        goal.delivery == GoalDelivery.Pending -> "Sending the goal to ${agentName(agent)}."
        goal.delivery == GoalDelivery.Failed -> goal.deliveryReason
        else -> null
    }
    GoalState.Paused -> goal.reason ?: "Paused"
    GoalState.Met -> goal.reportedAt?.let { "Goal met. ${agentName(agent)} reported it done at ${at(it)}." } ?: "Goal met. ${goal.reason.orEmpty()}".trim()
    GoalState.Stuck -> goal.reportedAt?.let { "Blocked. ${agentName(agent)} needs you: ${goal.reason.orEmpty().trimEnd('.')}. Continue once it is in place, or clear the goal." } ?: "Blocked. ${goal.reason.orEmpty()}".trim()
    GoalState.Stalled -> "${goal.reason.orEmpty()}. Resume to let ${agentName(agent)} try again, or send it a message first."
    GoalState.OverBudget -> goal.reason ?: "Over budget"
    GoalState.LimitReached -> goal.reason ?: "Limit reached"
}

private fun turnCount(count: Long): String = if (count == 2L) "two" else count.toString()

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

fun parseRepeatLimit(text: String): Int? = text.trim().toIntOrNull()?.takeIf { it >= 2 }
