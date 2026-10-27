package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.Exercise
import mycelium.state.DiffLine
import mycelium.state.Store
import mycelium.state.solutionLines

@Composable
fun SolutionDialog(store: Store, exercise: Exercise, onDismiss: () -> Unit) {
    val learning by store.learning.state.collectAsState()
    val files = learning.solutions[exercise.id]
    val colors = MyTheme.colors

    LaunchedEffect(exercise.id) {
        store.learning.loadSolution(exercise)
    }

    Dialog(onDismiss) {
        Label("A solution to ${exercise.title}", Modifier.semantics { heading() }, Tokens.Type.Title)

        if (exercise.sourceConversationId != null) {
            Label("What the agent wrote in ${exercise.sourceTitle ?: "the conversation"}, the conversation this exercise came from. Looking is not recorded.", color = colors.textMuted)
        }

        Column(Modifier.fillMaxWidth().heightIn(max = 420.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            when {
                files == null -> Label("Loading the solution.", color = colors.textMuted)
                files.isEmpty() -> Label("No solution for this exercise. It was not taken from an agent's code.", color = colors.textMuted)
                else -> for (file in files) {
                    val lines = solutionLines(file.diff)
                    val language = file.path.substringAfterLast('.', "")

                    Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, ControlShape)) {
                        Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                            Label(file.path, Modifier.weight(1f).semantics { heading() }, Tokens.Type.Code, maxLines = 1)
                            Label("+${lines.count { it.kind == DiffLine.Kind.Added }}", style = Tokens.Type.Code, color = colors.diffAdd)
                            Label("-${lines.count { it.kind == DiffLine.Kind.Removed }}", style = Tokens.Type.Code, color = colors.diffDel)
                        }

                        Hairline()

                        for (line in lines) {
                            DiffBodyRow(line, language, line.oldNumber == null && line.newNumber == null, false)
                        }
                    }
                }
            }
        }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            exercise.sourceConversationId?.let { id -> MyButton({ store.act { store.open(id) }; onDismiss() }, text = "Open the conversation", variant = ButtonVariant.Outline) }
            MyButton(onDismiss, text = "Close", muted = true)
        }
    }
}
