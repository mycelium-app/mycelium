package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import kotlin.time.Clock
import mycelium.api.Goal
import mycelium.api.GoalDelivery
import mycelium.api.GoalState
import mycelium.state.clockTime
import mycelium.state.agentName
import mycelium.state.goalCanNudgeNow
import mycelium.state.goalIsBlocked
import mycelium.state.goalIsHeld
import mycelium.state.goalIsWaiting
import mycelium.state.parseRepeatLimit
import mycelium.state.GoalLimits
import mycelium.state.formatTimeLeft
import mycelium.state.formatTokens
import mycelium.state.goalMessage
import mycelium.state.goalProgress
import mycelium.state.parseTimeLimit
import mycelium.state.parseTokenBudget
import mycelium.state.parseWindowLimit

data class GoalControls(
    val goal: Goal?,
    val onSet: (text: String, limits: GoalLimits) -> Unit,
    val onPause: () -> Unit,
    val onResume: () -> Unit,
    val onClear: () -> Unit,
    val onSendAgain: () -> Unit,
    val onNudge: () -> Unit = {},
    val onContinue: (note: String?) -> Unit = {},
    val agent: String = "",
)

@Composable
fun GoalBar(goal: Goal, controls: GoalControls) {
    val colors = MyTheme.colors
    val agent = controls.agent
    val held = goalIsHeld(goal)
    val attention = goalIsWaiting(goal) || goalIsBlocked(goal) && goal.reportedAt != null
    var continuing by remember { mutableStateOf(false) }
    val tint = when (goal.state) {
        GoalState.Active -> if (held || attention) colors.statusAttention else if (goal.delivery == GoalDelivery.Failed) colors.statusFailed else colors.statusWorking
        GoalState.Paused -> colors.textMuted
        GoalState.Met -> colors.accent
        GoalState.Stuck -> if (attention) colors.statusAttention else colors.statusFailed
        GoalState.Stalled, GoalState.OverBudget, GoalState.LimitReached -> colors.statusFailed
    }
    val actions: @Composable () -> Unit = {
        when (goal.state) {
            GoalState.Active -> if (goal.delivery == GoalDelivery.Failed) {
                MyButton(controls.onSendAgain, text = "Send again", description = "Send the goal to the agent again", size = ButtonSize.Sm, muted = true)
            } else {
                if (goalCanNudgeNow(goal)) {
                    MyButton(controls.onNudge, text = "Nudge now", description = "Nudge ${agentName(agent)} now instead of waiting", size = ButtonSize.Sm, muted = true)
                }

                MyButton(controls.onPause, text = "Pause", description = "Pause the goal", size = ButtonSize.Sm, muted = true)
            }
            GoalState.Paused, GoalState.Stalled -> MyButton(controls.onResume, text = "Resume", description = "Resume the goal", size = ButtonSize.Sm, muted = true)
            GoalState.Stuck -> MyButton({ continuing = true }, text = "Continue", description = "Continue the goal", size = ButtonSize.Sm, muted = true)
            else -> Unit
        }
    }
    val platform = LocalPlatform.current
    val message = goalMessage(goal, agent) { instant -> clockTime(instant) { millis -> platform?.localDateTime(millis) } }
    val now = rememberNow(ClockTickMillis, ticking = goal.timeLimitSeconds != null && !held)
    val progress = goalProgress(goal, now)

    Column(Modifier.fillMaxWidth().semantics { contentDescription = "Goal" }) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(MyIcon.Target, tint, Tokens.Size.IconSm)
            Label("Goal: ${goal.text}", Modifier.weight(1f), maxLines = 1)

            if (!LocalTouch.current) {
                Label(progress, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                actions()
            }

            MyButton(controls.onClear, icon = MyIcon.Close, description = "Clear the goal", size = ButtonSize.Sm, muted = true)
        }

        if (LocalTouch.current) {
            Row(
                Modifier.fillMaxWidth().padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(progress, Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                actions()
            }
        }

        if (message != null) {
            Label(message, Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1).semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, if (attention || held || goal.delivery == GoalDelivery.Failed || goal.state == GoalState.Stalled) tint else colors.textMuted)
        }

        Hairline()
    }

    if (continuing) {
        ContinueGoalDialog(agent, onContinue = { note -> controls.onContinue(note); continuing = false }, onDismiss = { continuing = false })
    }
}

@Composable
private fun ContinueGoalDialog(agent: String, onContinue: (String?) -> Unit, onDismiss: () -> Unit) {
    var note by remember { mutableStateOf("") }

    Dialog(onDismiss) {
        Label("Continue the goal", Modifier.semantics { heading() }, Tokens.Type.Title)
        TextInput(note, { note = it }, label = "Note for ${agentName(agent)} (optional)", onSubmit = { onContinue(note.trim().ifEmpty { null }) })

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton({ onContinue(note.trim().ifEmpty { null }) }, text = "Continue", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
fun GoalButton(controls: GoalControls) {
    var open by remember { mutableStateOf(false) }

    MyButton({ open = true }, icon = MyIcon.Target, description = if (controls.goal == null) "Set a goal" else "Edit the goal", selected = controls.goal != null, muted = controls.goal == null)

    if (open) {
        GoalDialog(controls.goal, onSet = { text, limits -> controls.onSet(text, limits); open = false }, onDismiss = { open = false })
    }
}

private const val ClockTickMillis = 30_000L

const val RepeatLimitError = "Enter how many turns, as a whole number like 3, or leave it empty."

@Composable
private fun GoalDialog(held: Goal?, onSet: (String, GoalLimits) -> Unit, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    var text by remember { mutableStateOf(held?.text.orEmpty()) }
    var budget by remember { mutableStateOf(held?.tokenBudget?.let { formatTokens(it) }.orEmpty()) }
    var timeLimit by remember { mutableStateOf(held?.timeLimitSeconds?.let { formatTimeLeft(it) }.orEmpty()) }
    var fiveHour by remember { mutableStateOf(held?.fiveHourLimit?.let { "$it%" }.orEmpty()) }
    var weekly by remember { mutableStateOf(held?.weeklyLimit?.let { "$it%" }.orEmpty()) }
    var repeatLimit by remember { mutableStateOf(held?.repeatLimit?.toString().orEmpty()) }
    var repeatError by remember { mutableStateOf<String?>(null) }
    var textError by remember { mutableStateOf<String?>(null) }
    var budgetError by remember { mutableStateOf<String?>(null) }
    var timeError by remember { mutableStateOf<String?>(null) }
    var fiveHourError by remember { mutableStateOf<String?>(null) }
    var weeklyError by remember { mutableStateOf<String?>(null) }

    fun submit() {
        val tokens = parseTokenBudget(budget)
        val seconds = parseTimeLimit(timeLimit)
        val fiveHourPercent = parseWindowLimit(fiveHour)
        val weeklyPercent = parseWindowLimit(weekly)
        val repeatTurns = parseRepeatLimit(repeatLimit)
        textError = if (text.isBlank()) "Enter the goal the agent should work towards." else null
        budgetError = if (budget.isNotBlank() && tokens == null) "Enter a token budget as a number, like 500k or 5M, or leave it empty." else null
        timeError = if (timeLimit.isNotBlank() && seconds == null) "Enter a time limit like 2h, 45m or 1h 30m, or leave it empty." else null
        fiveHourError = if (fiveHour.isNotBlank() && fiveHourPercent == null) "Enter a 5-hour window limit between 1 and 100 percent, like 80%, or leave it empty." else null
        weeklyError = if (weekly.isNotBlank() && weeklyPercent == null) "Enter a weekly window limit between 1 and 100 percent, like 80%, or leave it empty." else null
        repeatError = if (repeatLimit.isNotBlank() && repeatTurns == null) RepeatLimitError else null

        if (listOf(textError, budgetError, timeError, fiveHourError, weeklyError, repeatError).all { it == null }) {
            onSet(text.trim(), GoalLimits(tokens, seconds, fiveHourPercent, weeklyPercent, repeatTurns))
        }
    }

    Dialog(onDismiss) {
        Label("Set a goal", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label("After each turn Mycelium prompts the agent again, until the agent reports the goal done or blocked, or a limit is reached.", style = Tokens.Type.Meta, color = colors.textMuted)
        TextInput(text, { text = it }, label = "Goal", placeholder = "Every test passes on main, including e2e", error = textError, onSubmit = ::submit)
        TextInput(budget, { budget = it }, label = "Token budget", placeholder = "5M", error = budgetError, onSubmit = ::submit)
        TextInput(timeLimit, { timeLimit = it }, label = "Time limit", placeholder = "2h or 45m", error = timeError, onSubmit = ::submit)
        TextInput(fiveHour, { fiveHour = it }, label = "Stop when the account's 5-hour window reaches", placeholder = "80%", error = fiveHourError, onSubmit = ::submit)
        TextInput(weekly, { weekly = it }, label = "Stop when the account's weekly window reaches", placeholder = "80%", error = weeklyError, onSubmit = ::submit)
        TextInput(repeatLimit, { repeatLimit = it }, label = "Stop after this many turns end the same way", placeholder = "3", error = repeatError, hint = "Mycelium waits longer before each repeat, then stops and asks you. Leave empty for 3.", onSubmit = ::submit)

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::submit, text = "Set goal", variant = ButtonVariant.Outline)
        }
    }
}
