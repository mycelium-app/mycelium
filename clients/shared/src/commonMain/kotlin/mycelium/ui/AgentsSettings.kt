package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
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

@Composable
fun AgentsSettings(store: Store) {
    var onUsageLimit by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        store.act { onUsageLimit = store.api.usageLimitSetting().onUsageLimit }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        onUsageLimit?.let { chosen ->
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("When a usage limit is reached", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Segmented(
                    UsageLimitChoices.map { it.first },
                    UsageLimitChoices.first { it.second == chosen }.first,
                    { label ->
                        val picked = UsageLimitChoices.first { it.first == label }.second
                        onUsageLimit = picked
                        store.act { store.api.saveUsageLimitSetting(UsageLimitSetting(picked)) }
                    },
                    groupLabel = "When a usage limit is reached",
                )
            }
        }
    }
}
