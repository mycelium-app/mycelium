package mycelium.ui

import androidx.compose.runtime.Composable
import mycelium.state.Store

@Composable
fun AgentsSettings(store: Store) {
    Label("Agents", style = Tokens.Type.UiStrong)
}
