package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.Indexing
import mycelium.state.Store

@Composable
fun IndexingSettings(store: Store) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var indexing by remember { mutableStateOf<Indexing?>(null) }

    LaunchedEffect(state.conversations.size) {
        store.act { indexing = store.api.indexing() }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Label("Indexing", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Label("Node", Modifier.weight(2f), Tokens.Type.MetaStrong, colors.textMuted)
            Label("Transcripts", Modifier.weight(1f), Tokens.Type.MetaStrong, colors.textMuted)
            Label("Imported", Modifier.weight(1f), Tokens.Type.MetaStrong, colors.textMuted)
            Label("Conversations", Modifier.weight(1f), Tokens.Type.MetaStrong, colors.textMuted)
        }

        Hairline()

        for (row in indexing?.nodes.orEmpty()) {
            Row(
                Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(state.nodes[row.nodeId]?.name.orEmpty(), Modifier.weight(2f), maxLines = 1)
                Label(row.transcripts.toString(), Modifier.weight(1f), color = colors.textMuted)
                Label(row.imported.toString(), Modifier.weight(1f), color = colors.textMuted)
                Label(row.conversations.toString(), Modifier.weight(1f), color = colors.textMuted)
            }

            Hairline()
        }

        indexing?.let {
            Label("${it.entries} searchable messages across ${it.conversations} conversations", Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S2), Tokens.Type.Meta, colors.textMuted)
        }
    }
}
