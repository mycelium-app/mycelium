package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
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
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import mycelium.Platform
import mycelium.api.AppDeployment
import mycelium.api.AppDetail
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.addressLabel
import mycelium.state.addressUrl
import mycelium.state.ago
import mycelium.state.appRunning
import mycelium.state.commitLine
import mycelium.state.deploymentResultWord
import mycelium.state.formatDuration
import mycelium.state.mostRecentDeployment
import mycelium.state.triggeredByWord

const val AppOverviewTab = "Overview"
const val AppDeploymentsTab = "Deployments"
const val AppLogsTab = "Logs"
const val AppEnvironmentTab = "Environment"

private val AppTabs = listOf(AppOverviewTab, AppDeploymentsTab, AppLogsTab, AppEnvironmentTab)

private const val AppPollMillis = 3_000L

@Composable
fun AppPage(
    store: Store,
    state: AppState,
    platform: Platform,
    uuid: String,
    startTab: String,
    startDeployment: String?,
    onBack: () -> Unit,
    onOpenConversation: (String) -> Unit,
) {
    val colors = MyTheme.colors
    var app by remember(uuid) { mutableStateOf<AppDetail?>(null) }
    var loadFailure by remember(uuid) { mutableStateOf<String?>(null) }
    var deployments by remember(uuid) { mutableStateOf<List<AppDeployment>>(emptyList()) }
    var tab by remember(uuid, startTab) { mutableStateOf(startTab) }
    var logsFor by remember(uuid, startDeployment) { mutableStateOf(startDeployment) }
    var revision by remember(uuid) { mutableStateOf(0) }
    var deleting by remember { mutableStateOf(false) }
    val acting = rememberAction()
    val deploying = app?.status == "deploying"

    LaunchedEffect(uuid, revision) {
        loadFailure = attempt {
            app = store.api.app(uuid)
            deployments = store.api.appDeployments(uuid)
        }
    }

    LaunchedEffect(uuid, deploying) {
        while (deploying) {
            delay(AppPollMillis)
            attempt {
                app = store.api.app(uuid)
                deployments = store.api.appDeployments(uuid)
            }
        }
    }

    fun act(block: suspend () -> Unit) {
        acting {
            block()
            revision += 1
        }
    }

    val current = app

    Column(Modifier.fillMaxSize()) {
        Bar {
            BackButton("Apps", onBack, showText = true)

            if (current == null) {
                Label(loadFailure ?: "Loading the app.", color = if (loadFailure != null) colors.statusFailed else colors.textMuted)
            } else {
                Label("/", color = colors.textFaint)
                Label(current.name, Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)
                AppStatus(current.status)

                if (!LocalTouch.current) {
                    Box(Modifier.weight(1f))
                    AppActions(current, platform, ::act, store)
                }
            }
        }

        if (current == null) {
            return@Column
        }

        if (LocalTouch.current) {
            Box(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)) {
                AppActions(current, platform, ::act, store)
            }
        }

        acting.failure?.let { ErrorBanner(it, onDismiss = { acting.failure = null }, modifier = Modifier.padding(horizontal = Tokens.Space.S3)) }

        TabRow(AppTabs, tab, { tab = it }, groupLabel = "App sections", modifier = Modifier.padding(horizontal = Tokens.Space.S3))

        Hairline()

        if (tab == AppLogsTab) {
            AppLogsView(store, platform, current, deployments, logsFor, { logsFor = it }, Modifier.weight(1f).fillMaxWidth())
        } else {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                when (tab) {
                    AppOverviewTab -> AppOverview(current, state, platform, deployments, onOpenConversation, onAllDeployments = { tab = AppDeploymentsTab }, onDelete = { deleting = true }, onOpenLogs = { logsFor = it; tab = AppLogsTab })
                    AppDeploymentsTab -> {
                        Label("Deployments", Modifier.semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)

                        if (deployments.isEmpty()) {
                            Label("Nothing has been deployed yet.", color = colors.textMuted)
                        } else {
                            DeploymentsTable(deployments, state, onOpenConversation) { logsFor = it.uuid; tab = AppLogsTab }
                        }
                    }

                    else -> AppEnvironmentView(store, current, ::act)
                }
            }
        }
    }

    if (deleting) {
        DeleteAppDialog(current?.name.orEmpty(), current?.address, current?.repository, { deleting = false }) {
            deleting = false
            store.act {
                store.api.deleteApp(uuid)
                store.refreshApps()
                onBack()
            }
        }
    }
}

@Composable
private fun AppActions(app: AppDetail, platform: Platform, act: (suspend () -> Unit) -> Unit, store: Store) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
        app.address?.let { address ->
            MyButton({ platform.openUrl(addressUrl(address)) }, text = "Open", description = "Open ${addressLabel(address)}", variant = ButtonVariant.Outline)
        }

        MyButton({ act { store.api.redeployApp(app.uuid) } }, text = "Redeploy", variant = ButtonVariant.Outline)
        AppsStopStart(appRunning(app.status), { act { store.api.stopApp(app.uuid) } }, { act { store.api.startApp(app.uuid) } })
    }
}

@Composable
private fun AppOverview(
    app: AppDetail,
    state: AppState,
    platform: Platform,
    deployments: List<AppDeployment>,
    onOpenConversation: (String) -> Unit,
    onAllDeployments: () -> Unit,
    onDelete: () -> Unit,
    onOpenLogs: (String) -> Unit,
) {
    val colors = MyTheme.colors
    val source = listOfNotNull(app.repository?.let(::addressLabel), app.branch, app.baseDirectory?.takeIf { it.isNotBlank() && it != "/" }).joinToString(" · ")
    val location = listOfNotNull(app.project?.let { "$it project" }, app.server?.let { "$it server" }).joinToString(", ")
    val conversation = app.conversationId?.let { id -> state.conversations[id]?.let { id to it.title } }
    val latest = app.latestDeployment ?: mostRecentDeployment(deployments)

    Section("Details") {
        app.address?.let { address ->
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                Label("Address", Modifier.width(120.dp), Tokens.Type.Meta, colors.textMuted)
                MyButton({ platform.openUrl(addressUrl(address)) }, text = addressUrl(address), description = "Open ${addressLabel(address)}", size = ButtonSize.Sm, muted = true)
            }
        }

        if (source.isNotBlank()) {
            Fact("Source", source)
        }

        app.buildPack?.let { Fact("Build", it.replaceFirstChar { letter -> letter.uppercase() }) }

        if (location.isNotBlank()) {
            Fact("In Coolify", location)
        }

        conversation?.let { (id, title) ->
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                Label("Built in", Modifier.width(120.dp), Tokens.Type.Meta, colors.textMuted)
                MyButton({ onOpenConversation(id) }, text = title, description = "Open the conversation $title", size = ButtonSize.Sm, muted = true)
            }
        }

        app.createdAt?.let { created -> expiryDate(created)?.let { Fact("Created", it) } }
    }

    Section("Latest deployment") {
        if (latest == null) {
            Label("Nothing has been deployed yet.", color = colors.textMuted)
        } else {
            DeploymentsTable(listOf(latest), state, onOpenConversation) { onOpenLogs(it.uuid) }
            Row { MyButton(onAllDeployments, text = "All deployments", variant = ButtonVariant.Outline, size = ButtonSize.Sm) }
        }
    }

    Hairline()

    Row { MyButton(onDelete, text = "Delete app", variant = ButtonVariant.Danger) }
}

@Composable
internal fun DeploymentsTable(deployments: List<AppDeployment>, state: AppState, onOpenConversation: (String) -> Unit, onRow: (AppDeployment) -> Unit) {
    val colors = MyTheme.colors

    Table(
        listOf(
            Col("Started", 72.dp),
            Col("Commit", 140.dp, weight = 1f),
            Col("Triggered by", 96.dp, priority = 1),
            Col("Conversation", 120.dp, priority = 2),
            Col("Duration", 72.dp, numeric = true, priority = 1),
            Col("Result", 96.dp),
        ),
        deployments.size,
        { row, column ->
            val deployment = deployments[row]

            when (column) {
                0 -> TableCell.Text(ago(deployment.startedAt), color = colors.textMuted)
                1 -> TableCell.Text(commitLine(deployment).ifBlank { "No commit" }, Tokens.Type.Code)
                2 -> TableCell.Text(triggeredByWord(deployment.triggeredBy), color = colors.textMuted)
                3 -> {
                    val id = deployment.conversationId
                    val title = id?.let { state.conversations[it]?.title }

                    TableCell.Slot(measureAs = title ?: "None") {
                        if (id != null && title != null) {
                            MyButton({ onOpenConversation(id) }, text = title, description = "Open the conversation $title", size = ButtonSize.Sm, muted = true)
                        }
                    }
                }
                4 -> TableCell.Text(deployment.durationSeconds?.let { formatDuration(it * 1000) } ?: "", color = colors.textMuted)
                else -> TableCell.Slot(measureAs = "Cancelled", extra = 24.dp) { DeploymentResult(deployment.result) }
            }
        },
        lazy = false,
        rowKey = { deployments[it].uuid },
        onRow = { onRow(deployments[it]) },
        rowDescription = { "Open the build log for ${commitLine(deployments[it]).ifBlank { "this deployment" }}" },
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun DeploymentResult(result: String) {
    val colors = MyTheme.colors
    val (icon, tint) = when (result) {
        "deployed" -> MyIcon.Check to colors.statusDone
        "failed" -> MyIcon.Failed to colors.statusFailed
        "cancelled" -> MyIcon.Stop to colors.textMuted
        else -> MyIcon.Working to colors.statusWorking
    }

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, tint, Tokens.Size.IconSm)
        Label(deploymentResultWord(result), style = Tokens.Type.Meta, color = if (result == "failed") colors.statusFailed else colors.textMuted, maxLines = 1)
    }
}
