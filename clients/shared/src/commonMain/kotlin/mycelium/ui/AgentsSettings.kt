package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.UsageLimitSetting
import mycelium.state.Store

private val UsageLimitChoices = listOf("Ask me" to "ask", "Wait and resume automatically" to "resume")

private val SwitchChoice = "Continue on another account" to "switch"

@Composable
fun AgentsSettings(store: Store) {
    var onUsageLimit by remember { mutableStateOf<String?>(null) }
    val state by store.state.collectAsState()
    val choices = if (state.experiments.continueElsewhere) UsageLimitChoices + SwitchChoice else UsageLimitChoices

    LaunchedEffect(Unit) {
        store.act { onUsageLimit = store.api.usageLimitSetting().onUsageLimit }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        onUsageLimit?.let { chosen ->
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("When a usage limit is reached", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Segmented(
                    choices.map { it.first },
                    (choices.firstOrNull { it.second == chosen } ?: UsageLimitChoices.first { it.second == "resume" }).first,
                    { label ->
                        val picked = choices.first { it.first == label }.second
                        onUsageLimit = picked
                        store.act { store.api.saveUsageLimitSetting(UsageLimitSetting(picked)) }
                    },
                    groupLabel = "When a usage limit is reached",
                )
            }
        }
    }
}
