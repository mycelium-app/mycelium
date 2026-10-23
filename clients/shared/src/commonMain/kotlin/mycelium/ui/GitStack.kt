package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.GitStack
import mycelium.api.GitStackBranch

private val StackIndent = 16.dp

@Composable
fun GitStackTab(stack: GitStack?, working: String?, onSwitch: (String) -> Unit, onSync: () -> Unit, onPropose: () -> Unit) {
    val colors = MyTheme.colors

    if (stack == null) {
        Label("Loading the stack", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)

        return
    }

    Column(Modifier.fillMaxWidth().semantics { contentDescription = "Stack" }) {
        if (stack.gitTownAvailable) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(working.orEmpty(), Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                MyButton(onSync, text = "Sync stack", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }
        }

        if (stack.inferred) {
            Label("Git Town has no record of this stack, so it is worked out from which branches contain which.", Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted)
        }

        stack.pullRequestsProblem?.let {
            Label("Pull requests are not shown: $it", Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted)
        }

        for (branch in stack.branches) {
            StackRow(branch, stack, onSwitch, onPropose)
        }
    }
}

@Composable
private fun StackRow(branch: GitStackBranch, stack: GitStack, onSwitch: (String) -> Unit, onPropose: () -> Unit) {
    val colors = MyTheme.colors
    val uri = LocalUriHandler.current
    val request = branch.pullRequest

    Column(
        Modifier.fillMaxWidth().padding(start = Tokens.Space.S3 + StackIndent * branch.depth, end = Tokens.Space.S2, top = Tokens.Space.S1, bottom = Tokens.Space.S1),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
    ) {
        Row(Modifier.heightIn(min = rowHeight()), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
            Icon(if (branch.current) MyIcon.Check else MyIcon.Branch, if (branch.current) colors.accent else colors.textMuted, Tokens.Size.IconSm)
            Label(branch.name, Modifier.weight(1f, fill = false), if (branch.current) Tokens.Type.UiStrong else Tokens.Type.Ui, maxLines = 1)

            if (branch.current) {
                Tag("Current branch")
            }

            if (branch.main) {
                Tag("Main")
            }

            if (branch.needsSync) {
                Tag("Needs sync")
            }

            if (!branch.main && !branch.pushed) {
                Tag("Not pushed")
            }

            if (!branch.current) {
                Row(Modifier.weight(1f), horizontalArrangement = Arrangement.End) {
                    MyButton({ onSwitch(branch.name) }, text = "Switch", description = "Switch to ${branch.name}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }
            }
        }

        branch.parent?.let { parent ->
            val counts = if (branch.ahead == 0 && branch.behind == 0) "Up to date with $parent" else "${branch.ahead} ahead, ${branch.behind} behind $parent"

            Label("$counts · ${branch.subject}", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
        }

        if (request != null) {
            val number = request.label.filter { it.isDigit() }.ifEmpty { request.number.toString() }

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                MyButton(
                    { uri.openUri(request.url) },
                    text = "${request.label} ${request.state}",
                    description = "Open pull request $number: ${request.title}",
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                )
                Label(request.title, Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
            }
        } else if (stack.pullRequestsAvailable && !branch.main) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label("No pull request", style = Tokens.Type.Meta, color = colors.textMuted)

                if (branch.current && stack.gitTownAvailable) {
                    MyButton(onPropose, text = "Propose", description = "Propose ${branch.name}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }
            }
        }
    }
}
