package mycelium.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import mycelium.api.Project
import mycelium.state.AppState
import mycelium.state.Store

private const val MixedGroups = "Mixed groups"
private const val NoGroupName = "No group"

fun projectGroupLabel(state: AppState, projects: List<Project>): String {
    val shared = projects.map { it.groupId }.distinct()

    return if (shared.size > 1) MixedGroups else state.groups[shared.singleOrNull()]?.name ?: NoGroupName
}

@Composable
fun ProjectGroupMenu(store: Store, state: AppState, projects: List<Project>, name: String) {
    var open by remember { mutableStateOf(false) }
    var changingIcon by remember { mutableStateOf(false) }
    val shared = projects.map { it.groupId }.distinct()
    val current = shared.singleOrNull()
    val label = projectGroupLabel(state, projects)

    fun assign(groupId: String?) {
        open = false

        store.act {
            for (project in projects.filter { it.groupId != groupId }) {
                store.setProjectGroup(project.id, groupId)
            }
        }
    }

    val grouped = state.groups.isNotEmpty()

    Box {
        if (grouped) {
            MyButton({ open = true }, text = label, description = "Group of $name: $label", size = ButtonSize.Sm, muted = true, trailingIcon = MyIcon.ChevronDown, selected = open)
        } else {
            MyButton({ open = true }, icon = MyIcon.MoreVertical, description = "Options for $name", size = ButtonSize.Sm, muted = true, selected = open)
        }

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                if (grouped) {
                    MenuLabel("Group of $name")
                    MenuItem(NoGroupName, onClick = { assign(null) }, checked = shared.size == 1 && current == null)

                    for (group in state.groups.values.sortedBy { it.name.lowercase() }) {
                        MenuItem(group.name, onClick = { assign(group.id) }, checked = shared.size == 1 && current == group.id)
                    }

                    Hairline()
                }

                MenuItem("Change icon…", onClick = { open = false; changingIcon = true })
            }
        }
    }

    if (changingIcon) {
        ProjectIconDialog(store, projects, name, onDismiss = { changingIcon = false })
    }
}
