package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.state.Store

@Composable
fun ExperimentsSettings(store: Store) {
    val state by store.state.collectAsState()
    val held = state.experiments
    val choices = listOf(
        Experiment("Voice input", "Dictate into the composer. On Android and the web, audio may go to Google.", held.voice) { value -> store.updateExperiments { it.copy(voice = value) } },
    )

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("Experiments", Modifier.semantics { heading() }, Tokens.Type.Title)

        for (experiment in choices) {
            Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Checkbox(experiment.on, experiment.name, { experiment.save(!experiment.on) }, checkedState = "On", uncheckedState = "Off")
                Column(Modifier.padding(top = Tokens.Space.S1_5)) {
                    Label(experiment.name)
                    Label(experiment.description, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
                }
            }
        }
    }
}

private class Experiment(val name: String, val description: String, val on: Boolean, val save: (Boolean) -> Unit)
