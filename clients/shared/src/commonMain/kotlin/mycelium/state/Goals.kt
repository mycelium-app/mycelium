package mycelium.state

import mycelium.api.Goal
import mycelium.api.GoalState

const val GoalCheckKind = "check"
const val DefaultGoalBudget = "5M"

fun goalProgress(goal: Goal): String = "Turn ${goal.turns}  ·  ${formatTokens(goal.tokensUsed)} of ${formatTokens(goal.tokenBudget)} tokens"

fun goalMessage(goal: Goal): String? = when (goal.state) {
    GoalState.Active -> null
    GoalState.Paused -> goal.reason ?: "Paused"
    GoalState.Met -> "Goal met. ${goal.reason.orEmpty()}".trim()
    GoalState.Stuck -> "Goal stuck. ${goal.reason.orEmpty()}".trim()
    GoalState.OverBudget -> goal.reason ?: "Over budget"
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
