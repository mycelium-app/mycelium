package mycelium.ui

import androidx.compose.runtime.Composable
import mycelium.state.Store

@Composable
fun IndexingSettings(store: Store) {
    Label("Indexing", style = Tokens.Type.UiStrong)
}
