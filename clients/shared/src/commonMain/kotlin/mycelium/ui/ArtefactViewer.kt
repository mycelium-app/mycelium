package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Dp
import mycelium.Platform
import mycelium.api.Artifact
import mycelium.api.ArtifactKinds
import mycelium.state.AppState
import mycelium.state.MarkdownBlock
import mycelium.state.Store
import mycelium.state.ago
import mycelium.state.artifactKindName
import mycelium.state.artifactOpenUrl
import mycelium.state.parsedMarkdown

@Composable
fun ArtefactViewer(
    artifact: Artifact,
    store: Store,
    platform: Platform,
    modifier: Modifier = Modifier,
    onPopOut: (() -> Unit)? = null,
    onBack: (() -> Unit)? = null,
    onBackDescription: String = "Back to artefacts",
    trailing: @Composable RowScope.() -> Unit = {},
) {
    val colors = MyTheme.colors
    val appearance = LocalAppearance.current
    val accent = if (appearance.accent == Accent.System) "neutral" else appearance.accent.name.lowercase()
    val url = artifactOpenUrl(store.api.baseUrl, artifact, dark = appearance.dark, accent = accent)
    val token = platform.loadConnection()?.token

    Column(modifier) {
        Bar {
            if (onBack != null) {
                MyButton(onBack, icon = MyIcon.ChevronLeft, description = onBackDescription, muted = true)
            }

            Icon(artifactIcon(artifact.kind), colors.textMuted, Tokens.Size.IconSm)
            Label(artifact.title, Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 1)
            Label(artifactKindName(artifact.kind), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            trailing()
            MyButton({ platform.openUrl(url) }, icon = MyIcon.Globe, description = "Open in browser", muted = true)

            if (onPopOut != null) {
                MyButton(onPopOut, icon = MyIcon.Move, description = "Open full view", muted = true)
            }
        }

        if (artifact.kind == ArtifactKinds.Markdown) {
            val blocks by produceState<List<MarkdownBlock>?>(null, artifact.id, artifact.latestVersion) {
                value = store.artifactContent(artifact)?.let { parsedMarkdown(it.decodeToString()) }
            }
            val drawn = blocks

            if (drawn != null) {
                Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S3)) {
                    MarkdownBlocks(drawn)
                }
            } else {
                Box(Modifier.weight(1f).fillMaxWidth())
            }
        } else {
            platform.WebContent(url, token, Modifier.weight(1f).fillMaxWidth())
        }
    }
}

@Composable
fun ArtefactList(artifacts: List<Artifact>, onSelect: (Artifact) -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    if (artifacts.isEmpty()) {
        Label("No artefact in this conversation yet.", modifier.padding(Tokens.Space.S3), color = colors.textMuted)

        return
    }

    LazyColumn(modifier, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1), contentPadding = PaddingValues(Tokens.Space.S2)) {
        items(artifacts, key = { it.id }) { artifact ->
            Row(
                Modifier.fillMaxWidth().pressable({ onSelect(artifact) }, shape = BlockShape, description = "View ${artifact.title}").padding(Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(artifactIcon(artifact.kind), colors.textMuted, Tokens.Size.IconSm)

                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label(artifact.title, style = Tokens.Type.UiStrong, maxLines = 1)
                    Label("${artifactKindName(artifact.kind)}, edited ${ago(artifact.updatedAt)}", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }
            }
        }
    }
}

fun conversationArtefacts(state: AppState, conversationId: String): List<Artifact> =
    state.artifacts.values.filter { it.conversationId == conversationId }.sortedByDescending { it.updatedAt }

@Composable
fun ArtefactsPanel(
    store: Store,
    state: AppState,
    platform: Platform,
    conversationId: String,
    selected: String?,
    onSelect: (String?) -> Unit,
    onPopOut: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    LaunchedEffect(conversationId) {
        store.act { store.refreshArtifacts() }
    }

    val viewed = selected?.let { state.artifacts[it] }

    if (viewed == null) {
        ArtefactList(conversationArtefacts(state, conversationId), { onSelect(it.id) }, modifier)

        return
    }

    ArtefactViewer(
        viewed,
        store,
        platform,
        modifier,
        onPopOut = { onPopOut(viewed.id) },
        onBack = { onSelect(null) },
        onBackDescription = "Back to this conversation's artefacts",
    )
}

@Composable
fun ArtefactFullView(
    store: Store,
    state: AppState,
    platform: Platform,
    artifactId: String,
    onBack: () -> Unit,
    onOpenNodes: () -> Unit,
    onOpenConversation: (String) -> Unit,
) {
    LaunchedEffect(artifactId) {
        if (state.artifacts[artifactId] == null) {
            store.act { store.refreshArtifacts() }
        }
    }

    val artifact = state.artifacts[artifactId]

    if (artifact == null) {
        Column(Modifier.fillMaxSize()) {
            Bar {
                MyButton(onBack, icon = MyIcon.ChevronLeft, description = "Back to artefacts", muted = true)
                Label("Artefact not found", style = Tokens.Type.Title, maxLines = 1)
            }

            Label("This artefact no longer exists or is not available to you.", Modifier.padding(Tokens.Space.S4), color = MyTheme.colors.textMuted)
        }

        return
    }

    val attached = artifact.conversationId?.takeIf { state.conversations[it] != null }
    val compact = LocalTouch.current
    var chatShown by remember(artifactId) { mutableStateOf(attached != null && !compact) }
    var chatWidth by remember { mutableStateOf(Tokens.Size.SidePanel) }

    fun resizeChat(width: Dp) {
        chatWidth = width.coerceIn(Tokens.Size.SidePanelMin, Tokens.Size.SidePanelMax)
    }

    val toggle: @Composable RowScope.() -> Unit = {
        if (attached != null) {
            MyButton({ chatShown = !chatShown }, icon = MyIcon.Chat, description = if (chatShown) "Hide chat" else "Show chat", muted = true, selected = chatShown)
        }
    }

    Row(Modifier.fillMaxSize()) {
        if (!(compact && chatShown)) {
            ArtefactViewer(artifact, store, platform, Modifier.weight(1f).fillMaxHeight(), onBack = onBack, trailing = toggle)
        }

        if (attached != null && chatShown) {
            if (compact) {
                Column(Modifier.weight(1f).fillMaxHeight()) {
                    Bar {
                        MyButton({ chatShown = false }, icon = MyIcon.ChevronLeft, description = "Back to the artefact", muted = true)
                        Label(artifact.title, style = Tokens.Type.Title, maxLines = 1)
                    }

                    ConversationScreen(store, state, attached, onArchived = onBack, onOpenNodes = onOpenNodes, onOpenConversation = onOpenConversation, panels = false)
                }
            } else {
                ResizeHandle("Resize the chat") { moved -> resizeChat(chatWidth - moved) }

                Column(Modifier.width(chatWidth).fillMaxHeight()) {
                    ConversationScreen(store, state, attached, onArchived = onBack, onOpenNodes = onOpenNodes, onOpenConversation = onOpenConversation, panels = false)
                }
            }
        }
    }
}
