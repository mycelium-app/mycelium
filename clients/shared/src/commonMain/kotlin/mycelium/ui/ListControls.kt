package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.state.Choice

@Composable
fun ListSearch(
    value: String,
    onValueChange: (String) -> Unit,
    things: String,
    modifier: Modifier = Modifier,
    placeholder: String = "Search $things",
    mono: Boolean = false,
    onSubmit: (() -> Unit)? = null,
) {
    TextInput(value, onValueChange, label = "Search $things", placeholder = placeholder, showLabel = false, mono = mono, modifier = modifier.width(Tokens.Size.FormColumn), leadingIcon = MyIcon.Search, onSubmit = onSubmit)
}

@Composable
fun EmptyState(text: String, modifier: Modifier = Modifier, alert: Boolean = false, action: (@Composable () -> Unit)? = null) {
    Column(
        modifier.padding(horizontal = Tokens.Space.S4).then(if (alert) Modifier.semantics { liveRegion = LiveRegionMode.Assertive } else Modifier),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3, Alignment.CenterVertically),
    ) {
        Label(text, color = MyTheme.colors.textMuted)
        action?.invoke()
    }
}

@Composable
fun LoadFailedState(things: String, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    EmptyState("Could not load $things. Check your connection, then try again.", modifier, alert = true) {
        MyButton(onRetry, text = "Retry", variant = ButtonVariant.Outline, description = "Retry loading $things")
    }
}

@Composable
fun ProjectFilter(projects: List<Choice>, selectedId: String?, onSelect: (String?) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val chosen = projects.firstOrNull { it.id == selectedId }
    val label = chosen?.name ?: "All projects"

    Box {
        MyButton({ open = true }, text = label, description = "Project: $label", variant = ButtonVariant.Outline, size = ButtonSize.Sm, muted = chosen == null, trailingIcon = MyIcon.ChevronDown, selected = open)

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                MenuLabel("Project")
                MenuItem("All projects", onClick = { open = false; onSelect(null) }, checked = selectedId == null)

                for (project in projects) {
                    MenuItem(project.name, onClick = { open = false; onSelect(project.id) }, checked = project.id == selectedId)
                }
            }
        }
    }
}

@Composable
fun ListFrame(things: String, loaded: Boolean, failed: Boolean, rows: Int, onRetry: () -> Unit, empty: String, content: @Composable () -> Unit) {
    when {
        failed && rows == 0 -> LoadFailedState(things, onRetry)
        loaded && rows == 0 -> EmptyState(empty, Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S4))
        else -> content()
    }
}
