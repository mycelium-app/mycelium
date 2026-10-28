package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.Platform
import mycelium.api.AppSummary
import mycelium.state.AppState
import mycelium.state.CoolifyKind
import mycelium.state.Store
import mycelium.state.addressLabel
import mycelium.state.addressUrl
import mycelium.state.ago
import mycelium.state.appMatches
import mycelium.state.appRunning
import mycelium.state.appStatusWord

private const val AppsList = "apps"

val LocalAppsOpener = compositionLocalOf<(() -> Unit)?> { null }

val LocalAppOpener = compositionLocalOf<((String, String?) -> Unit)?> { null }

@Composable
internal fun AppStatus(status: String) {
    val colors = MyTheme.colors
    val word = appStatusWord(status)
    val (icon, tint) = when (status) {
        "running" -> MyIcon.Check to colors.statusDone
        "deploying" -> MyIcon.Working to colors.statusWorking
        "failed" -> MyIcon.Failed to colors.statusFailed
        else -> MyIcon.Stop to colors.textMuted
    }

    Row(Modifier.semantics(mergeDescendants = true) { contentDescription = "Status: $word" }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, tint, Tokens.Size.IconSm)
        Label(word, style = Tokens.Type.Meta, color = if (status == "failed") colors.statusFailed else colors.textMuted, maxLines = 1)
    }
}

@Composable
internal fun AppsStopStart(running: Boolean, onStop: () -> Unit, onStart: () -> Unit) {
    MyButton(if (running) onStop else onStart, text = if (running) "Stop" else "Start", variant = ButtonVariant.Outline)
}

@Composable
internal fun DeleteAppDialog(name: String, address: String?, repository: String?, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    val stops = address?.let { " ${addressLabel(it)} stops answering." }.orEmpty()
    val code = repository?.let { " The code in ${addressLabel(it)} stays." } ?: " The code stays in git."

    ConfirmDialog("Delete $name?", "Stops it and removes it, its deployments and its environment from Coolify.$stops$code", "Delete app", onDismiss, onConfirm)
}

@Composable
fun AppsScreen(store: Store, state: AppState, platform: Platform, onOpenApp: (String) -> Unit, onOpenConversation: (String) -> Unit, onNewConversation: () -> Unit) {
    val muted = MyTheme.colors.textMuted
    var query by remember { mutableStateOf("") }
    var deleting by remember { mutableStateOf<AppSummary?>(null) }
    val loadFailures by store.loadFailures.collectAsState()
    val load = { store.load(AppsList) { store.refreshApps() } }

    LaunchedEffect(Unit) {
        load()
    }

    val apps = state.apps
    val shown = apps.orEmpty().filter { appMatches(it, query) }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Apps")
        }

        if (apps == null) {
            if (AppsList in loadFailures) {
                LoadFailedState("apps", load, Modifier.fillMaxSize())
            }
        } else if (apps.isEmpty()) {
            val target = state.chatToolServers.firstOrNull { it.kind == CoolifyKind }?.let { account -> settingText(account, "project_name")?.let { project -> settingText(account, "server_name")?.let { server -> " in $project on $server" } } }.orEmpty()

            EmptyState("No apps yet", Modifier.fillMaxSize()) {
                Label("Ask an agent to build one. It creates the app$target once you approve, and the app appears here.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
                MyButton(onNewConversation, text = "New conversation", variant = ButtonVariant.Outline)
            }
        } else {
            Row(Modifier.fillMaxWidth().padding(Tokens.Space.S3)) {
                ListSearch(query, { query = it }, "apps")
            }

            Hairline()

            if (shown.isEmpty()) {
                EmptyState("No app matches.", Modifier.fillMaxWidth().padding(Tokens.Space.S4))
            } else {
                Column(Modifier.fillMaxSize().verticalScrollWithBar().padding(Tokens.Space.S3)) {
                    Table(
                        listOf(
                            Col("App", 120.dp, weight = 1f),
                            Col("Status", 96.dp, preferred = 112.dp),
                            Col("Address", 140.dp, priority = 1),
                            Col("Last deploy", 88.dp, priority = 1),
                            Col("Built in", 120.dp, priority = 2),
                            ActionColumn,
                        ),
                        shown.size,
                        { row, column ->
                            val app = shown[row]

                            when (column) {
                                0 -> TableCell.Text(app.name, Tokens.Type.UiStrong)
                                1 -> TableCell.Slot(measureAs = "Deploying", extra = 24.dp) { AppStatus(app.status) }
                                2 -> TableCell.Slot(measureAs = app.address?.let(::addressLabel) ?: "None") {
                                    app.address?.let { address ->
                                        MyButton({ platform.openUrl(addressUrl(address)) }, text = addressLabel(address), description = "Open ${addressLabel(address)}", size = ButtonSize.Sm, muted = true)
                                    }
                                }
                                3 -> TableCell.Text(app.lastDeployAt?.let(::ago) ?: "Never", color = muted)
                                4 -> {
                                    val title = app.conversationId?.let { state.conversations[it]?.title }

                                    TableCell.Slot(measureAs = title ?: "None") {
                                        val id = app.conversationId

                                        if (id != null && title != null) {
                                            MyButton({ onOpenConversation(id) }, text = title, description = "Open the conversation $title", size = ButtonSize.Sm, muted = true)
                                        }
                                    }
                                }
                                else -> TableCell.Slot(measureAs = "") {
                                    AppRowMenu(store, app, platform, onDelete = { deleting = app })
                                }
                            }
                        },
                        lazy = false,
                        rowKey = { shown[it].uuid },
                        onRow = { onOpenApp(shown[it].uuid) },
                        rowDescription = { "Open ${shown[it].name}" },
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
            }
        }
    }

    deleting?.let { app ->
        DeleteAppDialog(app.name, app.address, app.repository, { deleting = null }) {
            deleting = null
            store.act {
                store.api.deleteApp(app.uuid)
                store.refreshApps()
            }
        }
    }
}

@Composable
private fun AppRowMenu(store: Store, app: AppSummary, platform: Platform, onDelete: () -> Unit) {
    RowMenu(app.name) { close ->
        app.address?.let { address ->
            MenuItem("Open", onClick = { close(); platform.openUrl(addressUrl(address)) }, leading = MyIcon.Globe)
        }

        MenuItem("Redeploy", onClick = { close(); store.act { store.api.redeployApp(app.uuid); store.refreshApps() } }, leading = MyIcon.Refresh)

        if (appRunning(app.status)) {
            MenuItem("Stop", onClick = { close(); store.act { store.api.stopApp(app.uuid); store.refreshApps() } }, leading = MyIcon.Stop)
        } else {
            MenuItem("Start", onClick = { close(); store.act { store.api.startApp(app.uuid); store.refreshApps() } }, leading = MyIcon.Play)
        }

        MenuItem("Delete", onClick = { close(); onDelete() }, leading = MyIcon.Close)
    }
}
