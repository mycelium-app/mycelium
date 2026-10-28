package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import kotlinx.coroutines.delay
import mycelium.Platform
import mycelium.api.AppDeployment
import mycelium.api.AppDetail
import mycelium.api.AppLogs
import mycelium.state.BuildLogs
import mycelium.state.Choice
import mycelium.state.RuntimeLogs
import mycelium.state.Store
import mycelium.state.ago
import mycelium.state.deploymentResultWord
import mycelium.state.filteredLogLines
import mycelium.state.mostRecentDeployment

private const val BuildTitle = "Build"
private const val RuntimeTitle = "Runtime"
private const val FollowMillis = 3_000L
private const val LogLines = 300

private fun highlighted(line: String, query: String, mark: Color): AnnotatedString =
    buildAnnotatedString {
        append(line)

        val needle = query.trim()
        var from = 0

        while (needle.isNotEmpty()) {
            val at = line.indexOf(needle, from, ignoreCase = true)

            if (at < 0) {
                break
            }

            addStyle(SpanStyle(background = mark), at, at + needle.length)
            from = at + needle.length
        }
    }

@Composable
internal fun AppLogsView(
    store: Store,
    platform: Platform,
    app: AppDetail,
    deployments: List<AppDeployment>,
    deployment: String?,
    onDeployment: (String?) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = MyTheme.colors
    var kind by remember(app.uuid, deployment) { mutableStateOf(if (deployment != null) BuildTitle else RuntimeTitle) }
    var query by remember { mutableStateOf("") }
    var follow by remember { mutableStateOf(false) }
    var logs by remember(app.uuid) { mutableStateOf<AppLogs?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var refresh by remember { mutableStateOf(0) }
    val scroll = rememberScrollState()
    val building = kind == BuildTitle
    val chosen = if (building) deployment ?: mostRecentDeployment(deployments)?.uuid else null

    LaunchedEffect(app.uuid, kind, chosen, refresh) {
        failure = attempt {
            logs = store.api.appLogs(app.uuid, if (building) BuildLogs else RuntimeLogs, chosen, LogLines)
        }
    }

    LaunchedEffect(follow) {
        while (follow) {
            delay(FollowMillis)
            refresh += 1
        }
    }

    val lines = logs?.lines.orEmpty()
    val shown = filteredLogLines(lines, query)

    LaunchedEffect(shown.size, follow) {
        if (follow) {
            scroll.scrollTo(scroll.maxValue)
        }
    }

    LaunchedEffect(follow) {
        var last = scroll.value

        snapshotFlow { scroll.value to scroll.isScrollInProgress }.collect { (value, scrolling) ->
            if (follow && scrolling && value < last) {
                follow = false
            }

            last = value
        }
    }

    Column(modifier.padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
            Segmented(listOf(BuildTitle, RuntimeTitle), kind, { kind = it }, "Log kind")

            if (building && deployments.isNotEmpty()) {
                OptionMenu(
                    "Deployment",
                    deployments.sortedByDescending { it.startedAt }.map { Choice(it.uuid, "${ago(it.startedAt)} ${it.commit?.take(7).orEmpty()} ${deploymentResultWord(it.result)}".replace("  ", " ")) },
                    chosen,
                    { onDeployment(it) },
                    size = ButtonSize.Sm,
                )
            }

            ListSearch(
                query,
                { query = it },
                "logs",
                Modifier.onPreviewKeyEvent { event ->
                    val clears = event.type == KeyEventType.KeyDown && event.key == Key.Escape && query.isNotEmpty()

                    if (clears) {
                        query = ""
                    }

                    clears
                },
                mono = true,
            )

            MyButton({ follow = !follow }, Modifier.semantics { selected = follow }, text = "Follow", icon = MyIcon.ArrowDown, variant = ButtonVariant.Outline, selected = follow, stateDescription = if (follow) "On" else "Off")
            MyButton({ platform.copyToClipboard(shown.joinToString("\n")) }, text = "Copy", icon = MyIcon.Copy, variant = ButtonVariant.Outline, description = "Copy the log")
        }

        val current = logs
        val unavailable = current != null && !current.available

        when {
            failure != null -> Label(failure.orEmpty(), color = colors.statusFailed, modifier = Modifier.semantics { liveRegion = LiveRegionMode.Assertive })
            unavailable -> Label(current?.message ?: "Runtime logs are not available for this app.", color = colors.textMuted)
            current == null -> Label("Loading the log.", color = colors.textMuted)
            lines.isEmpty() -> Label("No log lines yet.", color = colors.textMuted)
            shown.isEmpty() -> Label("No line matches.", color = colors.textMuted)
            else -> Column(
                Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .semantics {
                        contentDescription = if (building) "Build log" else "Runtime log"

                        if (follow) {
                            liveRegion = LiveRegionMode.Polite
                        }
                    }
                    .verticalScrollWithBar(scroll),
            ) {
                for (line in shown) {
                    BasicText(highlighted(line, query, colors.diffAddBg), Modifier.fillMaxWidth(), style = Tokens.Type.Code.themed().copy(color = colors.text))
                }
            }
        }
    }
}
