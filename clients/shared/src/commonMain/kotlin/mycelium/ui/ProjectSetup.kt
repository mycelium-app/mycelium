package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.api.NodeSummary
import mycelium.api.Project
import mycelium.state.AppState

sealed interface ProjectSetup {
    val project: Project
    val node: NodeSummary

    data class Running(override val project: Project, override val node: NodeSummary) : ProjectSetup

    data class Failed(override val project: Project, override val node: NodeSummary, val log: String) : ProjectSetup
}

data class SetupActions(
    val onCancel: () -> Unit,
    val onRetry: () -> Unit,
    val onStartOnAnyNode: () -> Unit,
    val onRemove: () -> Unit,
)

fun checkoutOn(state: AppState, project: Project, nodeId: String): Project? = when {
    project.nodeId == nodeId -> project
    project.repository == null -> null
    else -> state.projects.values.firstOrNull { it.nodeId == nodeId && it.forgeId == project.forgeId && it.repository == project.repository }
}

fun cloneable(project: Project): Boolean = project.forgeId != null && project.repository != null

@Composable
fun SetupStrip(setup: ProjectSetup, actions: SetupActions) {
    val colors = MyTheme.colors
    val shape = RoundedCornerShape(topStart = Tokens.Radius.Block, topEnd = Tokens.Radius.Block)
    var showingLog by remember(setup) { mutableStateOf(false) }

    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = Tokens.Space.S4)
            .background(colors.bgPanel, shape)
            .border(Tokens.Size.Hairline, colors.border, shape)
            .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1)
            .semantics { liveRegion = LiveRegionMode.Polite },
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
    ) {
        when (setup) {
            is ProjectSetup.Running -> {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    Icon(MyIcon.Working, colors.statusWorking, Tokens.Size.IconSm)
                    Label("Cloning ${setup.project.name} on ${setup.node.name}", Modifier.weight(1f), Tokens.Type.MetaStrong)
                    MyButton(actions.onCancel, text = "Cancel", description = "Cancel cloning ${setup.project.name} on ${setup.node.name}", size = ButtonSize.Sm, muted = true)
                }

                IndeterminateProgress("Cloning ${setup.project.name} on ${setup.node.name}")
            }

            is ProjectSetup.Failed -> {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
                    Icon(MyIcon.Failed, colors.statusFailed, Tokens.Size.IconSm, Modifier.padding(top = Tokens.Space.S1))

                    Column(Modifier.weight(1f).padding(vertical = Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                        Label("Setup failed on ${setup.node.name}", Modifier.semantics { heading() }, Tokens.Type.MetaStrong)
                        Label(setup.log, style = Tokens.Type.Meta, color = colors.textMuted)
                    }

                    SplitButton(
                        "Retry setup",
                        actions.onRetry,
                        listOf(
                            SplitAlternative("Show the full log", { showingLog = true }),
                            SplitAlternative("Start on Any node instead", actions.onStartOnAnyNode),
                            SplitAlternative("Remove the checkout", actions.onRemove),
                        ),
                        variant = ButtonVariant.Outline,
                        size = ButtonSize.Sm,
                    )
                }
            }
        }
    }

    if (showingLog) {
        Dialog(onDismiss = { showingLog = false }) {
            Label("Setup log for ${setup.project.name} on ${setup.node.name}", Modifier.semantics { heading() }, Tokens.Type.Title)
            Label(if (setup is ProjectSetup.Failed) setup.log else "", style = Tokens.Type.Code)
            MyButton({ showingLog = false }, text = "Close", variant = ButtonVariant.Outline)
        }
    }
}
