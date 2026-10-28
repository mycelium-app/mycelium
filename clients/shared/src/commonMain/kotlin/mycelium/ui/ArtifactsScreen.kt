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
import androidx.compose.runtime.collectAsState
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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import org.jetbrains.compose.resources.decodeToImageBitmap
import mycelium.Platform
import mycelium.api.Artifact
import mycelium.api.ArtifactOrigin
import mycelium.api.Project
import mycelium.api.ArtifactKinds
import mycelium.api.Me
import mycelium.state.MarkdownBlock
import mycelium.state.AllArtifacts
import mycelium.state.artifactInGroup
import mycelium.state.ArtifactPreview
import mycelium.state.AppState
import mycelium.state.ArtifactKindFilters
import mycelium.state.ArtifactTabs
import mycelium.state.Store
import mycelium.state.ago
import mycelium.state.artifactKindName
import mycelium.state.artifactProjects
import mycelium.state.artifactOpenUrl
import mycelium.state.artifactPreview
import mycelium.state.PreviewLines
import mycelium.state.parsedMarkdown
import mycelium.state.visibleArtifacts

private const val AnyKind = "All kinds"

private const val ArtifactsList = "artefacts"

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
    ArtifactKinds.Drawing -> MyIcon.PenTool
    ArtifactKinds.Typst -> MyIcon.TypeText
    ArtifactKinds.Doc -> MyIcon.FileText
    ArtifactKinds.Slides -> MyIcon.Presentation
    ArtifactKinds.Dashboard -> MyIcon.Chart
    ArtifactKinds.Motion -> MyIcon.Film
    ArtifactKinds.DesignSystem -> MyIcon.Palette
    else -> MyIcon.Paperclip
}

@Composable
fun ArtifactsScreen(store: Store, state: AppState, me: Me, platform: Platform, onOpen: (String) -> Unit = {}) {
    val colors = MyTheme.colors
    var tab by remember { mutableStateOf(AllArtifacts) }
    var kindName by remember { mutableStateOf(AnyKind) }
    var query by remember { mutableStateOf("") }
    var projectId by remember { mutableStateOf<String?>(null) }
    var creating by remember { mutableStateOf(false) }
    val loadFailures by store.loadFailures.collectAsState()
    val load = { store.load(ArtifactsList) { store.refreshArtifacts(); store.refreshArtifactAccessRequests() } }

    LaunchedEffect(Unit) {
        load()
    }

    val kind = ArtifactKindFilters.firstOrNull { it.second == kindName }?.first
    val group = LocalGroupFilter.current
    val shown = visibleArtifacts(state.artifacts.values.filter { artifactInGroup(it, state, group) }, tab, me.id, kind, query, projectId)

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Artefacts")
            Box(Modifier.weight(1f))
            MyButton({ creating = true }, text = "New artefact", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
        }

        FlowRow(
            Modifier.fillMaxWidth().padding(Tokens.Space.S3),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            ListSearch(query, { query = it }, "artefacts")
            Segmented(ArtifactTabs, tab, { tab = it }, groupLabel = "Whose artefacts")
            KindFilter(kindName) { kindName = it }
            ProjectFilter(artifactProjects(state.artifacts.values), projectId) { projectId = it }
        }

        Hairline()

        if (creating) {
            NewArtifactDialog(store, state.projects.values, state.artifacts.values, onCreated = { onOpen(it.id) }, onDismiss = { creating = false })
        }

        for (request in state.artifactAccessRequests.values.filter { it.state == "requested" }) {
            ArtifactAccessRequestCard(request, store, state)
        }

        if (ArtifactsList in loadFailures && state.artifacts.isEmpty()) {
            LoadFailedState("artefacts", load, Modifier.fillMaxSize())
        } else if (shown.isEmpty()) {
            EmptyState(if (state.artifacts.isEmpty()) "No artefacts. Ask an agent to create a design or a page and it appears here." else "No artefact matches.", Modifier.fillMaxSize())
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
private fun KindFilter(kindName: String, onSelect: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }

    Box {
        MyButton({ open = true }, text = kindName, description = "Kind: $kindName", variant = ButtonVariant.Outline, size = ButtonSize.Sm, muted = kindName == AnyKind, trailingIcon = MyIcon.ChevronDown, selected = open)

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                MenuLabel("Kind")
                MenuItem(AnyKind, onClick = { open = false; onSelect(AnyKind) }, checked = kindName == AnyKind)

                for ((kind, name) in ArtifactKindFilters) {
                    MenuItem(name, onClick = { open = false; onSelect(name) }, checked = kindName == name, leading = artifactIcon(kind))
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
private fun ArtifactOriginLine(origin: ArtifactOrigin, state: AppState) {
    val colors = MyTheme.colors
    val project = origin.projectId?.let { id -> state.projects[id] ?: Project(id, "", origin.projectName.orEmpty(), "", createdAt = "", icon = origin.projectIcon) }
    val description = "From ${origin.conversationTitle}${project?.let { " in ${it.name}" }.orEmpty()}"

    Row(Modifier.semantics(mergeDescendants = true) { contentDescription = description }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        if (project != null) {
            ProjectIcon(project, Tokens.Size.IconSm)
            Label(project.name, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            Label("·", style = Tokens.Type.Meta, color = colors.textFaint)
        } else {
            Icon(MyIcon.Chat, colors.textMuted, Tokens.Size.IconSm)
        }

        Label(origin.conversationTitle, Modifier.weight(1f, fill = false), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
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
                artifact.origin?.let { ArtifactOriginLine(it, state) }
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

        RenameDialog("Rename artefact", "Artefact name", title, { title = it }, save, onDismiss = { renaming = false })
    }

    if (deleting) {
        ConfirmDialog(
            "Delete ${artifact.title}?",
            "Its versions, comments and data are removed. This cannot be undone.",
            "Delete artefact",
            onDismiss = { deleting = false },
            onConfirm = { deleting = false; store.act { store.deleteArtifact(artifact) } },
        )
    }
}
