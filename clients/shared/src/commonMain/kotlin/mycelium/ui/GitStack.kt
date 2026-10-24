package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.GitStack
import mycelium.api.GitStackBranch

private val StackGutter = 16.dp

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

        stack.branches.forEachIndexed { index, branch ->
            StackRow(branch, stack, first = index == 0, last = index == stack.branches.lastIndex, onSwitch, onPropose)
        }
    }
}

@Composable
private fun StackRow(branch: GitStackBranch, stack: GitStack, first: Boolean, last: Boolean, onSwitch: (String) -> Unit, onPropose: () -> Unit) {
    val colors = MyTheme.colors
    val uri = LocalUriHandler.current
    val request = branch.pullRequest
    val lineHeight = rowHeight()
    val dotCentre = Tokens.Space.S1 + lineHeight / 2
    val dotGap = Tokens.Size.Dot / 2 + Tokens.Space.S0_5
    val connector = colors.border

    Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min).padding(horizontal = Tokens.Space.S3)) {
        Box(
            Modifier.width(StackGutter).fillMaxHeight().drawBehind {
                val x = size.width / 2
                val centre = dotCentre.toPx()
                val gap = dotGap.toPx()

                if (!first) {
                    drawLine(connector, Offset(x, 0f), Offset(x, centre - gap), Tokens.Size.Hairline.toPx())
                }

                if (!last) {
                    drawLine(connector, Offset(x, centre + gap), Offset(x, size.height), Tokens.Size.Hairline.toPx())
                }
            },
        ) {
            Box(Modifier.padding(top = dotCentre - Tokens.Size.Dot / 2).align(Alignment.TopCenter)) {
                Dot(if (branch.current) colors.accent else colors.textMuted, hollow = !branch.current)
            }
        }

        Column(
            Modifier.weight(1f).padding(start = Tokens.Space.S1, top = Tokens.Space.S1, bottom = Tokens.Space.S1),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
        ) {
            Row(Modifier.fillMaxWidth().heightIn(min = lineHeight), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                    Label(branch.name, Modifier.weight(1f, fill = false), Tokens.Type.UiStrong, maxLines = 1)

                    if (branch.parent != null) {
                        Label("+${branch.ahead} \u2212${branch.behind}", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
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
                }

                if (branch.current) {
                    Tag("Current branch")
                } else {
                    MyButton({ onSwitch(branch.name) }, text = "Switch", description = "Switch to ${branch.name}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }
            }

            if (branch.subject.isNotEmpty()) {
                Label(branch.subject, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
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
}
