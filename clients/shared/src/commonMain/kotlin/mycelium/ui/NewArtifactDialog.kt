package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.api.Artifact
import mycelium.api.ArtifactKinds
import mycelium.api.CreateArtifactRequest
import mycelium.api.Project
import mycelium.state.Choice
import mycelium.state.NewArtifactKinds
import mycelium.state.Store
import mycelium.state.designSystemsOf
import mycelium.state.newArtifactNameProblem

private const val NoProject = ""

private const val NoDesignSystem = ""

@Composable
fun NewArtifactDialog(store: Store, projects: Collection<Project>, artifacts: Collection<Artifact>, onCreated: (Artifact) -> Unit, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    var kind by remember { mutableStateOf(NewArtifactKinds.first().first) }
    var name by remember { mutableStateOf("") }
    var projectId by remember { mutableStateOf(NoProject) }
    var designSystemId by remember { mutableStateOf(NoDesignSystem) }
    var nameProblem by remember { mutableStateOf<String?>(null) }
    val nameFocus = remember { FocusRequester() }
    val runner = rememberAction()
    val designSystems = designSystemsOf(artifacts)

    fun create() {
        val problem = newArtifactNameProblem(kind, name)

        nameProblem = problem

        if (problem != null) {
            return
        }

        runner {
            val created = store.createArtifact(
                CreateArtifactRequest(
                    kind = kind,
                    title = name.trim(),
                    projectId = projectId.ifEmpty { null },
                    designSystemId = if (kind == ArtifactKinds.Design) designSystemId.ifEmpty { null } else null,
                ),
            )

            onCreated(created)
            onDismiss()
        }
    }

    Dialog(onDismiss) {
        Label("New artefact", Modifier.semantics { heading() }, Tokens.Type.Title)

        nameProblem?.let { problem ->
            Column(
                Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.statusFailed, ControlShape).padding(Tokens.Space.S3).semantics { liveRegion = LiveRegionMode.Assertive },
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            ) {
                Label("There is a problem", Modifier.semantics { heading() }, Tokens.Type.UiStrong, colors.statusFailed)
                MyButton({ runCatching { nameFocus.requestFocus() } }, text = problem, variant = ButtonVariant.Ghost, description = "Go to Name: $problem")
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Kind", style = Tokens.Type.MetaStrong, color = colors.textMuted)

            for ((id, label, hint) in NewArtifactKinds) {
                val selected = id == kind

                Row(
                    Modifier
                        .fillMaxWidth()
                        .pressable({ kind = id; nameProblem = null }, role = Role.RadioButton, shape = ControlShape, description = "$label. $hint", stateDescription = if (selected) "Selected" else null)
                        .padding(Tokens.Space.S2),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(artifactIcon(id), if (selected) colors.text else colors.textMuted, Tokens.Size.IconSm)
                    Label(label, style = if (selected) Tokens.Type.UiStrong else Tokens.Type.Ui)
                    Label(hint, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }
            }
        }

        TextInput(name, { name = it }, label = "Name", error = nameProblem, onSubmit = ::create, focusRequester = nameFocus)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            OptionMenu("Project", listOf(Choice(NoProject, "No project")) + projects.sortedBy { it.name.lowercase() }.map { Choice(it.id, it.name) }, projectId, { projectId = it }, fill = true)
            Label("Conversations in the project can open and edit it.", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        if (kind == ArtifactKinds.Design) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                OptionMenu("Design system", listOf(Choice(NoDesignSystem, "None")) + designSystems.map { Choice(it.id, it.title) }, designSystemId, { designSystemId = it }, fill = true)
                Label("Agents build the design from its tokens and components. You can change it later.", style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::create, text = "Create", variant = ButtonVariant.Outline)
        }
    }
}
