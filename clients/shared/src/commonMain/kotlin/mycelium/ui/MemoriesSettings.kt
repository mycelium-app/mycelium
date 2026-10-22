package mycelium.ui

import androidx.compose.runtime.Composable
import mycelium.state.Store

@Composable
fun MemoriesSettings(store: Store) {
    Label("Memories", style = Tokens.Type.UiStrong)
}
