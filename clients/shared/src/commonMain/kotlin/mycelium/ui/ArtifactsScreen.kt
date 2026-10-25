package mycelium.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import org.jetbrains.compose.resources.decodeToImageBitmap
import mycelium.Platform
import mycelium.api.Artifact
import mycelium.api.ArtifactKinds
import mycelium.api.Me
import mycelium.state.MarkdownBlock
import mycelium.state.AllArtifacts
import mycelium.state.ArtifactPreview
import mycelium.state.AppState
import mycelium.state.ArtifactKindFilters
import mycelium.state.ArtifactTabs
import mycelium.state.Store
import mycelium.state.ago
import mycelium.state.artifactKindName
import mycelium.state.artifactOpenUrl
import mycelium.state.artifactPreview
import mycelium.state.PreviewLines
import mycelium.state.parsedMarkdown
import mycelium.state.visibleArtifacts

private const val AnyKind = "All kinds"

private val CardMinWidth = 260.dp

private const val PreviewAspectRatio = 16f / 10f

private val PreviewIconSize = 32.dp

private const val PreviewTextScale = 12f / 14f

private const val MaxDecodedPreviews = 200

private val decodedPreviews = LinkedHashMap<String, ImageBitmap>()

private val PreviewShape = RoundedCornerShape(topStart = Tokens.Radius.Block, topEnd = Tokens.Radius.Block)

internal fun artifactIcon(kind: String): MyIcon = when (kind) {
    ArtifactKinds.Design -> MyIcon.Layers
    ArtifactKinds.Page -> MyIcon.Globe
    ArtifactKinds.Markdown -> MyIcon.File
    else -> MyIcon.Paperclip
}

@Composable
fun ArtifactsScreen(store: Store, state: AppState, me: Me, platform: Platform, onOpen: (String) -> Unit = {}) {
    val colors = MyTheme.colors
    var tab by remember { mutableStateOf(AllArtifacts) }
    var kindName by remember { mutableStateOf(AnyKind) }
    var query by remember { mutableStateOf("") }

    LaunchedEffect(Unit) {
        store.act { store.refreshArtifacts(); store.refreshArtifactAccessRequests() }
    }

    val kind = ArtifactKindFilters.firstOrNull { it.second == kindName }?.first
    val shown = visibleArtifacts(state.artifacts.values, tab, me.id, kind, query)

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Artefacts")
        }

        FlowRow(
            Modifier.fillMaxWidth().padding(Tokens.Space.S3),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            Segmented(ArtifactTabs, tab, { tab = it }, groupLabel = "Whose artefacts")
            Segmented(listOf(AnyKind) + ArtifactKindFilters.map { it.second }, kindName, { kindName = it }, groupLabel = "Kind")
            TextInput(query, { query = it }, label = "Search artefacts", placeholder = "Search artefacts", showLabel = false, modifier = Modifier.width(Tokens.Size.FormColumn), leadingIcon = MyIcon.Search)
        }

        Hairline()

        for (request in state.artifactAccessRequests.values.filter { it.state == "requested" }) {
            ArtifactAccessRequestCard(request, store, state)
        }

        if (shown.isEmpty()) {
            val text = if (state.artifacts.isEmpty()) "No artefact yet. Ask an agent to create a design or a page and it appears here." else "No artefact matches."

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
                    ArtifactCard(artifact, store, state, platform, mine = artifact.owner == me.id, onOpen = { onOpen(artifact.id) })
                }
            }
        }
    }
}

@Composable
private fun ArtifactPreviewArea(artifact: Artifact, store: Store) {
    val colors = MyTheme.colors
    val preview = artifactPreview(artifact)
    val pictureKey = "${preview.name}:${artifact.id}:${artifact.thumbnailVersion}:${artifact.latestVersion}"
    val picture by produceState<ImageBitmap?>(decodedPreviews[pictureKey], pictureKey) {
        if (value != null) {
            return@produceState
        }

        val bytes = when (preview) {
            ArtifactPreview.Snapshot -> store.artifactThumbnail(artifact)
            ArtifactPreview.Picture -> store.artifactContent(artifact)
            else -> null
        }
        val decoded = bytes?.let { runCatching { it.decodeToImageBitmap() }.getOrNull() }

        if (decoded != null) {
            if (decodedPreviews.size >= MaxDecodedPreviews) {
                decodedPreviews.remove(decodedPreviews.keys.first())
            }

            decodedPreviews[pictureKey] = decoded
        }

        value = decoded
    }
    val text by produceState<List<MarkdownBlock>?>(null, artifact.id, artifact.latestVersion) {
        val markdown = if (preview == ArtifactPreview.Text) store.artifactContent(artifact)?.decodeToString() else null

        value = markdown?.let { parsedMarkdown(it.lines().take(PreviewLines).joinToString("\n")) }
    }
    val drawn = picture
    val excerpt = text

    Box(Modifier.fillMaxWidth().aspectRatio(PreviewAspectRatio).clip(PreviewShape).background(colors.bgPanel), contentAlignment = Alignment.Center) {
        if (drawn != null) {
            Image(drawn, contentDescription = null, Modifier.fillMaxSize(), alignment = Alignment.TopCenter, contentScale = ContentScale.Crop, filterQuality = FilterQuality.High)
        } else if (excerpt != null) {
            val density = LocalDensity.current

            CompositionLocalProvider(LocalDensity provides Density(density.density * PreviewTextScale, density.fontScale)) {
                Box(Modifier.fillMaxSize().padding(Tokens.Space.S2)) {
                    MarkdownBlocks(excerpt)
                }
            }

            Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0.6f to Color.Transparent, 1f to colors.bgPanel)))
        } else {
            Icon(artifactIcon(artifact.kind), colors.textMuted, PreviewIconSize)
        }
    }
}

@Composable
private fun ArtifactCard(artifact: Artifact, store: Store, state: AppState, platform: Platform, mine: Boolean, onOpen: () -> Unit) {
    val colors = MyTheme.colors
    var access by remember(artifact.id) { mutableStateOf(false) }
    var renaming by remember(artifact.id) { mutableStateOf(false) }
    var deleting by remember(artifact.id) { mutableStateOf(false) }

    Card {
        ArtifactPreviewArea(artifact, store)

        Row(Modifier.padding(Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Column(
                Modifier
                    .weight(1f)
                    .pressable(onOpen, shape = BlockShape, description = "Open ${artifact.title}")
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
                MenuItem("Open", onClick = { close(); onOpen() }, leading = MyIcon.Layers)
                MenuItem("Open in browser", onClick = { close(); platform.navigate(artifactOpenUrl(store.api.baseUrl, artifact)) }, leading = MyIcon.Globe)
                MenuItem("Copy link", onClick = { close(); platform.copyToClipboard(artifact.url ?: artifactOpenUrl(store.api.baseUrl, artifact)) }, leading = MyIcon.Copy)

                if (artifact.isPublic) {
                    MenuItem("Copy private link", onClick = { close(); platform.copyToClipboard(artifactOpenUrl(store.api.baseUrl, artifact)) }, leading = MyIcon.Key)
                }

                if (mine) {
                    MenuItem(if (artifact.isPublic) "Make private" else "Make public", onClick = { close(); store.act { store.setArtifactPublic(artifact, !artifact.isPublic) } }, leading = MyIcon.Key)
                    MenuItem("Manage access", onClick = { close(); access = true }, leading = MyIcon.Key)
                    MenuItem("Rename", onClick = { close(); renaming = true }, leading = MyIcon.Pencil)
                    MenuItem("Delete", onClick = { close(); deleting = true }, leading = MyIcon.Close)
                }
            }
        }
    }

    if (access) {
        ArtifactAccessDialog(artifact, store, state) { access = false }
    }

    if (renaming) {
        var title by remember { mutableStateOf(artifact.title) }
        val save = {
            renaming = false
            store.act { store.renameArtifact(artifact, title.trim()) }
        }

        Dialog(onDismiss = { renaming = false }) {
            Label("Rename artefact", Modifier.semantics { heading() }, Tokens.Type.Title)
            TextInput(title, { title = it }, label = "Artefact name", onSubmit = save)

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
                MyButton({ deleting = false; store.act { store.deleteArtifact(artifact) } }, text = "Delete artefact", variant = ButtonVariant.Danger)
            }
        }
    }
}
