package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
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
import mycelium.api.Goal
import mycelium.api.GoalState
import mycelium.state.DefaultGoalBudget
import mycelium.state.goalMessage
import mycelium.state.goalProgress
import mycelium.state.parseTokenBudget

data class GoalControls(
    val goal: Goal?,
    val onSet: (text: String, tokenBudget: Long) -> Unit,
    val onPause: () -> Unit,
    val onResume: () -> Unit,
    val onClear: () -> Unit,
)

@Composable
fun GoalBar(goal: Goal, onPause: () -> Unit, onResume: () -> Unit, onClear: () -> Unit) {
    val colors = MyTheme.colors
    val tint = when (goal.state) {
        GoalState.Active -> colors.statusWorking
        GoalState.Paused -> colors.textMuted
        GoalState.Met -> colors.accent
        GoalState.Stuck, GoalState.OverBudget -> colors.statusFailed
    }
    val actions: @Composable () -> Unit = {
        when (goal.state) {
            GoalState.Active -> MyButton(onPause, text = "Pause", description = "Pause the goal", size = ButtonSize.Sm, muted = true)
            GoalState.Paused -> MyButton(onResume, text = "Resume", description = "Resume the goal", size = ButtonSize.Sm, muted = true)
            else -> Unit
        }
    }
    val message = goalMessage(goal)

    Column(Modifier.fillMaxWidth().semantics { contentDescription = "Goal" }) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(MyIcon.Target, tint, Tokens.Size.IconSm)
            Label("Goal: ${goal.text}", Modifier.weight(1f), maxLines = 1)

            if (!LocalTouch.current) {
                Label(goalProgress(goal), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                actions()
            }

            MyButton(onClear, icon = MyIcon.Close, description = "Clear the goal", size = ButtonSize.Sm, muted = true)
        }

        if (LocalTouch.current) {
            Row(
                Modifier.fillMaxWidth().padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(goalProgress(goal), Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                actions()
            }
        }

        if (message != null) {
            Label(message, Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1).semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
        }

        Hairline()
    }
}

@Composable
fun GoalButton(controls: GoalControls) {
    var open by remember { mutableStateOf(false) }

    MyButton({ open = true }, icon = MyIcon.Target, description = if (controls.goal == null) "Set a goal" else "Edit the goal", selected = controls.goal != null, muted = controls.goal == null)

    if (open) {
        GoalDialog(controls.goal, onSet = { text, budget -> controls.onSet(text, budget); open = false }, onDismiss = { open = false })
    }
}

@Composable
private fun GoalDialog(held: Goal?, onSet: (String, Long) -> Unit, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    var text by remember { mutableStateOf(held?.text.orEmpty()) }
    var budget by remember { mutableStateOf(held?.tokenBudget?.toString() ?: DefaultGoalBudget) }
    var textError by remember { mutableStateOf<String?>(null) }
    var budgetError by remember { mutableStateOf<String?>(null) }

    fun submit() {
        val tokens = parseTokenBudget(budget)
        textError = if (text.isBlank()) "Enter the goal the agent should work towards." else null
        budgetError = if (tokens == null) "Enter a token budget as a number, like 500k or 5M." else null

        if (textError == null && tokens != null) {
            onSet(text.trim(), tokens)
        }
    }

    Dialog(onDismiss) {
        Label("Set a goal", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label("The agent keeps taking turns until a separate check says the goal is met or stuck, or the budget is used up. Sending a message pauses it.", style = Tokens.Type.Meta, color = colors.textMuted)
        TextInput(text, { text = it }, label = "Goal", placeholder = "Every test passes on main, including e2e", error = textError, onSubmit = ::submit)
        TextInput(budget, { budget = it }, label = "Token budget", error = budgetError, onSubmit = ::submit)

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::submit, text = "Set goal", variant = ButtonVariant.Outline)
        }
    }
}
