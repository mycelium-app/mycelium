package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.Platform
import mycelium.api.Artifact
import mycelium.api.ArtifactKinds
import mycelium.api.Me
import mycelium.state.AllArtifacts
import mycelium.state.AppState
import mycelium.state.ArtifactKindFilters
import mycelium.state.ArtifactTabs
import mycelium.state.Store
import mycelium.state.ago
import mycelium.state.artifactKindName
import mycelium.state.artifactOpenUrl
import mycelium.state.visibleArtifacts

private const val AnyKind = "All kinds"

private val CardMinWidth = 260.dp

internal fun artifactIcon(kind: String): MyIcon = when (kind) {
    ArtifactKinds.Design -> MyIcon.Layers
    ArtifactKinds.Page -> MyIcon.Globe
    ArtifactKinds.Markdown -> MyIcon.File
    else -> MyIcon.Paperclip
}

@Composable
fun ArtifactsScreen(store: Store, state: AppState, me: Me, platform: Platform) {
    val colors = MyTheme.colors
    var tab by remember { mutableStateOf(AllArtifacts) }
    var kindName by remember { mutableStateOf(AnyKind) }
    var query by remember { mutableStateOf("") }

    LaunchedEffect(Unit) {
        store.act { store.refreshArtifacts() }
    }

    val kind = ArtifactKindFilters.firstOrNull { it.second == kindName }?.first
    val shown = visibleArtifacts(state.artifacts.values, tab, me.id, kind, query)

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Artifacts")
        }

        FlowRow(
            Modifier.fillMaxWidth().padding(Tokens.Space.S3),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            Segmented(ArtifactTabs, tab, { tab = it }, groupLabel = "Whose artifacts")
            Segmented(listOf(AnyKind) + ArtifactKindFilters.map { it.second }, kindName, { kindName = it }, groupLabel = "Kind")
            TextInput(query, { query = it }, label = "Search artifacts", placeholder = "Search artifacts", showLabel = false, modifier = Modifier.width(Tokens.Size.FormColumn), leadingIcon = MyIcon.Search)
        }

        Hairline()

        if (shown.isEmpty()) {
            val text = if (state.artifacts.isEmpty()) "No artifact yet. Ask an agent to create a design or a page and it appears here." else "No artifact matches."

            Label(text, Modifier.padding(Tokens.Space.S4), color = colors.textMuted)
        } else {
            LazyVerticalGrid(
                GridCells.Adaptive(CardMinWidth),
                Modifier.fillMaxSize(),
                contentPadding = PaddingValues(Tokens.Space.S3),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
            ) {
                items(shown, key = { it.id }) { artifact ->
                    ArtifactCard(artifact, store, platform, mine = artifact.owner == me.id)
                }
            }
        }
    }
}

@Composable
private fun ArtifactCard(artifact: Artifact, store: Store, platform: Platform, mine: Boolean) {
    val colors = MyTheme.colors
    var renaming by remember(artifact.id) { mutableStateOf(false) }
    var deleting by remember(artifact.id) { mutableStateOf(false) }

    Card {
        Row(Modifier.padding(Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Column(
                Modifier
                    .weight(1f)
                    .pressable({ platform.navigate(artifactOpenUrl(store.api.baseUrl, artifact)) }, shape = BlockShape, description = "Open ${artifact.title}")
                    .padding(Tokens.Space.S1),
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
            ) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                    Icon(artifactIcon(artifact.kind), colors.textMuted, Tokens.Size.IconSm)
                    Label(artifactKindName(artifact.kind), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)

                    if (artifact.isPublic) {
                        Tag("Public")
                    }
                }

                Label(artifact.title, style = Tokens.Type.UiStrong, maxLines = 2)
                Label("Edited ${ago(artifact.updatedAt)}", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            RowMenu(artifact.title) { close ->
                MenuItem("Open", onClick = { close(); platform.navigate(artifactOpenUrl(store.api.baseUrl, artifact)) }, leading = MyIcon.Globe)
                MenuItem("Copy link", onClick = { close(); platform.copyToClipboard(artifact.url ?: artifactOpenUrl(store.api.baseUrl, artifact)) }, leading = MyIcon.Copy)

                if (mine) {
                    MenuItem(if (artifact.isPublic) "Make private" else "Make public", onClick = { close(); store.act { store.setArtifactPublic(artifact, !artifact.isPublic) } }, leading = MyIcon.Key)
                    MenuItem("Rename", onClick = { close(); renaming = true }, leading = MyIcon.Pencil)
                    MenuItem("Delete", onClick = { close(); deleting = true }, leading = MyIcon.Close)
                }
            }
        }
    }

    if (renaming) {
        var title by remember { mutableStateOf(artifact.title) }
        val save = {
            renaming = false
            store.act { store.renameArtifact(artifact, title.trim()) }
        }

        Dialog(onDismiss = { renaming = false }) {
            Label("Rename artifact", Modifier.semantics { heading() }, Tokens.Type.Title)
            TextInput(title, { title = it }, label = "Artifact name", onSubmit = save)

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
                MyButton({ renaming = false }, text = "Cancel", muted = true)
                MyButton(save, text = "Save", variant = ButtonVariant.Outline)
            }
        }
    }

    if (deleting) {
        Dialog(onDismiss = { deleting = false }) {
            Label("Delete ${artifact.title}?", Modifier.semantics { heading() }, Tokens.Type.Title)
            Label("Its versions, comments and data are removed. This cannot be undone.", style = Tokens.Type.Meta, color = colors.textMuted)

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
                MyButton({ deleting = false }, text = "Cancel", muted = true)
                MyButton({ deleting = false; store.act { store.deleteArtifact(artifact) } }, text = "Delete artifact", variant = ButtonVariant.Danger)
            }
        }
    }
}
