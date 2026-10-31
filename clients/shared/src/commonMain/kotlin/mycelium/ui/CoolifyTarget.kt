package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
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
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import mycelium.api.ChatToolServer
import mycelium.api.CoolifyOptions
import mycelium.api.CoolifyTargetRequest
import mycelium.state.Choice
import mycelium.state.Store
import mycelium.state.wildcardLabel

private const val WholeInstance = ""

internal fun settingText(account: ChatToolServer, key: String): String? = account.settings?.get(key)?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotBlank() }

internal fun exampleAddress(wildcard: String?): String? = wildcardLabel(wildcard)?.let { "harbour-status.${it.removePrefix("*.")}" }

@Composable
internal fun CoolifyTargetFields(options: CoolifyOptions, project: String, server: String?, onProject: (String) -> Unit, onServer: (String) -> Unit) {
    val colors = MyTheme.colors
    val example = exampleAddress(options.servers.firstOrNull { it.uuid == server }?.wildcardDomain)

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Label("Project", style = Tokens.Type.MetaStrong, color = colors.textMuted)
        OptionMenu("Project", listOf(Choice(WholeInstance, "Whole instance")) + options.projects.map { Choice(it.uuid, it.name) }, project, onProject, label = "Choose a project")
    }

    if (project != WholeInstance) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Server", style = Tokens.Type.MetaStrong, color = colors.textMuted)
            OptionMenu("Server", options.servers.map { Choice(it.uuid, it.name) }, server, onServer, label = "Choose a server")
            Label(if (example != null) "Apps get an address under its wildcard domain, like $example." else "Apps get an address under its wildcard domain.", style = Tokens.Type.Meta, color = colors.textMuted)
        }
    }
}

@Composable
internal fun WhereAppsGo(account: ChatToolServer, onChange: () -> Unit) {
    val opener = LocalAppsOpener.current

    Section("Where apps go") {
        Fact("Project", settingText(account, "project_name") ?: "Whole instance")
        settingText(account, "server_name")?.let { Fact("Server", it) }
        wildcardLabel(settingText(account, "wildcard_domain"))?.let { Fact("Addresses", it) }

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onChange, text = "Change where apps go", variant = ButtonVariant.Outline)
            opener?.let { open -> MyButton(open, text = "Open Apps", variant = ButtonVariant.Outline) }
        }
    }
}

@Composable
internal fun ChangeWhereAppsGoDialog(store: Store, account: ChatToolServer, onDismiss: () -> Unit, onSaved: (ChatToolServer) -> Unit) {
    val runner = rememberAction()
    var options by remember { mutableStateOf<CoolifyOptions?>(null) }
    var project by remember { mutableStateOf(settingText(account, "project_uuid") ?: WholeInstance) }
    var server by remember { mutableStateOf(settingText(account, "server_uuid")) }
    var loadFailure by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(account.id) {
        loadFailure = attempt { options = store.api.coolifyOptions(account.id) }
    }

    fun save() {
        val chosenServer = server

        if (project != WholeInstance && chosenServer == null) {
            runner.failure = "Choose a server."

            return
        }

        runner {
            val request = if (project == WholeInstance) CoolifyTargetRequest() else CoolifyTargetRequest(project, chosenServer)

            onSaved(store.api.setCoolifyTarget(account.id, request))
        }
    }

    Dialog(onDismiss) {
        Label("Change where apps go", Modifier.semantics { heading() }, Tokens.Type.Title)

        val loaded = options

        if (loaded != null) {
            CoolifyTargetFields(loaded, project, server, { project = it }, { server = it })
        } else {
            Label(loadFailure ?: "Loading projects and servers.", style = Tokens.Type.Meta, color = if (loadFailure != null) MyTheme.colors.statusFailed else MyTheme.colors.textMuted)
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}
