package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.Artifact
import mycelium.state.Store

@Composable
fun DesignSystemDialog(store: Store, design: Artifact, designSystems: List<Artifact>, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    var chosen by remember(design.id) { mutableStateOf(design.designSystemId) }
    val runner = rememberAction(design.id)
    val name = designSystems.firstOrNull { it.id == chosen }?.title

    Dialog(onDismiss) {
        Label("Design system for ${design.title}", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label("Agents use its tokens and components when they change this design. Artboards keep their look until an agent next updates them.", style = Tokens.Type.Meta, color = colors.textMuted)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            MenuItem("None", onClick = { chosen = null }, checked = chosen == null)

            for (system in designSystems) {
                MenuItem(system.title, onClick = { chosen = system.id }, checked = chosen == system.id)
            }
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton({ runner { store.setArtifactDesignSystem(design, chosen); onDismiss() } }, text = "Save", variant = ButtonVariant.Outline, description = if (name == null) "Save, no design system" else "Save, use $name")
        }
    }
}
