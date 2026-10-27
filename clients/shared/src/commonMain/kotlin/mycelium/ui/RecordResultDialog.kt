package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.state.Puzzle
import mycelium.state.RecordInput
import mycelium.state.Store
import mycelium.state.recordHint
import mycelium.state.recordInput
import mycelium.state.recordLabel

@Composable
fun RecordResultDialog(store: Store, puzzle: Puzzle, onDismiss: () -> Unit) {
    var score by remember { mutableStateOf("") }
    var time by remember { mutableStateOf("") }
    var scoreError by remember { mutableStateOf<String?>(null) }
    var timeError by remember { mutableStateOf<String?>(null) }
    val runner = rememberAction()
    val colors = MyTheme.colors

    val save: () -> Unit = {
        when (val input = puzzle.recordInput(score, time)) {
            is RecordInput.Valid -> {
                scoreError = null
                timeError = null
                runner {
                    store.wellness.recordManualResult(puzzle.id, input.result)
                    onDismiss()
                }
            }
            is RecordInput.Invalid -> {
                scoreError = input.scoreError
                timeError = input.timeError
            }
        }
    }

    Dialog(onDismiss) {
        Label("Record ${puzzle.name} for today", Modifier.semantics { heading() }, Tokens.Type.Title)

        val problems = listOfNotNull(scoreError, timeError)

        if (problems.isNotEmpty()) {
            Column(Modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Assertive }, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("There is a problem", Modifier.semantics { heading() }, Tokens.Type.UiStrong, colors.statusFailed)

                for (problem in problems) {
                    Label(problem, color = colors.statusFailed)
                }
            }
        }

        TextInput(score, { score = it }, label = puzzle.recordLabel(), hint = puzzle.recordHint(), error = scoreError, onSubmit = { save() })
        TextInput(time, { time = it }, label = "Time taken (optional)", hint = "Minutes and seconds, like 4:30", error = timeError, onSubmit = { save() })

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton({ save() }, text = "Save", variant = ButtonVariant.Primary)
        }
    }
}
