package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.api.NodeSummary
import mycelium.api.Project

@Composable
fun ProjectBreadcrumb(project: Project?, projects: List<Project>, nodes: Map<String, NodeSummary>, onSelect: (String?) -> Unit, setUpFolder: String? = null, onSetUp: () -> Unit = {}) {
    var open by remember { mutableStateOf(false) }
    val name = project?.name ?: "No project"

    Box {
        Row(
            Modifier
                .heightIn(min = touchMin(Tokens.Size.ControlSm))
                .pressable({ open = true }, description = "Project: $name", shape = ControlShape)
                .padding(horizontal = Tokens.Space.S1_5),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (project != null) {
                ProjectIcon(project)
            } else {
                Icon(MyIcon.Folder, MyTheme.colors.textMuted, Tokens.Size.IconSm)
            }

            Label(name, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted, maxLines = 1)
            Icon(MyIcon.ChevronDown, MyTheme.colors.textMuted, Tokens.Size.IconSm)
        }

        if (open) {
            ProjectMenu(projects, nodes, project?.id, "Project", onDismiss = { open = false }, setUpFolder = setUpFolder, onSetUp = { open = false; onSetUp() }) {
                open = false
                onSelect(it)
            }
        }
    }
}
