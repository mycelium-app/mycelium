package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.compositionLocalOf
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
import mycelium.api.Artifact
import mycelium.api.ArtifactAccessRequest
import mycelium.api.ArtifactLink
import mycelium.api.LinkArtifactRequest
import mycelium.state.AppState
import mycelium.state.Store

class ArtifactAccessControls(val store: Store, val state: AppState)

val LocalArtifactAccessControls = compositionLocalOf<ArtifactAccessControls?> { null }

@Composable
fun ArtifactAccessDialog(artifact: Artifact, store: Store, state: AppState, onDismiss: () -> Unit) {
    var links by remember(artifact.id) { mutableStateOf<List<ArtifactLink>>(emptyList()) }
    var targetKind by remember { mutableStateOf("Project") }
    var selected by remember { mutableStateOf<String?>(null) }
    var permission by remember { mutableStateOf("View") }
    var query by remember { mutableStateOf("") }
    var choosing by remember { mutableStateOf(false) }
    val targets = if (targetKind == "Project") state.projects.values.map { it.id to it.name } else state.conversations.values.filter { it.projectId == null }.map { it.id to it.title.ifEmpty { "Untitled conversation" } }
    val chosen = targets.firstOrNull { it.first == selected }

    LaunchedEffect(artifact.id, artifact.updatedAt) {
        store.act { links = store.api.artifactLinks(artifact.id) }
    }

    Dialog(onDismiss) {
        Label("${artifact.title}: access", Modifier.semantics { heading() }, Tokens.Type.Title)

        for (link in links) {
            val name = link.projectId?.let { state.projects[it]?.name ?: "Project $it" } ?: link.conversationId?.let { state.conversations[it]?.title ?: "Conversation $it" }.orEmpty()

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label(name, Modifier.weight(1f), maxLines = 2)
                Label(link.permission.replaceFirstChar { it.uppercase() }, style = Tokens.Type.Meta)
                MyButton({ store.act { store.api.unlinkArtifact(artifact.id, link.id); links = store.api.artifactLinks(artifact.id) } }, text = "Remove", description = "Remove access for $name", muted = true)
            }
        }

        Segmented(listOf("Project", "Conversation"), targetKind, { targetKind = it; selected = null }, groupLabel = "Access target")
        TextInput(query, { query = it }, label = "Search projects or conversations")
        Box {
            MyButton({ choosing = true }, text = chosen?.second ?: "Choose ${targetKind.lowercase()}", trailingIcon = MyIcon.ChevronDown)

            if (choosing) {
                MenuPopup({ choosing = false }) {
                    for ((id, name) in targets.filter { it.second.contains(query, ignoreCase = true) }.sortedBy { it.second }) {
                        MenuItem(name, onClick = { selected = id; choosing = false }, checked = selected == id)
                    }
                }
            }
        }
        Segmented(listOf("View", "Edit"), permission, { permission = it }, groupLabel = "Artefact permission")
        val requests = state.artifactAccessRequests.values.filter { it.artifactId == artifact.id && it.state == "requested" }

        if (requests.isNotEmpty()) {
            Label("Access requests", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            for (request in requests) {
                ArtifactAccessRequestCard(request, store, state)
            }
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton(onDismiss, text = "Close", muted = true)
            MyButton({
                store.act {
                    val target = chosen ?: error("Choose a project or conversation to grant access.")
                    store.api.linkArtifact(artifact.id, LinkArtifactRequest(projectId = target.first.takeIf { targetKind == "Project" }, conversationId = target.first.takeIf { targetKind == "Conversation" }, permission = permission.lowercase()))
                    links = store.api.artifactLinks(artifact.id)
                }
            }, text = "Add access", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
fun ArtifactAccessRequestCard(request: ArtifactAccessRequest, store: Store, state: AppState) {
    var project by remember(request.id) { mutableStateOf(false) }
    val conversation = state.conversations[request.conversationId]
    val title = state.artifacts[request.artifactId]?.title ?: "Artefact"

    Card(Modifier.padding(Tokens.Space.S2)) {
        Column(Modifier.padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("${conversation?.title ?: "Conversation"} asks to ${request.permission} $title", style = Tokens.Type.UiStrong)
            Label(request.reason)

            if (conversation?.projectId != null) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(project, "Grant access to its project", { project = !project }, checkedState = "Project access", uncheckedState = "Conversation access")
                    Label("Grant to its project")
                }
            }

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
                MyButton({ store.act { store.decideArtifactAccessRequest(request, false) } }, text = "Decline", muted = true)
                MyButton({ store.act { store.decideArtifactAccessRequest(request, true, project) } }, text = "Approve", variant = ButtonVariant.Outline)
            }
        }
    }
}
