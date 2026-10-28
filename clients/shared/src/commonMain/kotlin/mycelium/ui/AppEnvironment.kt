package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
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
import mycelium.api.AppDetail
import mycelium.api.AppEnv
import mycelium.api.PutAppEnvRequest
import mycelium.state.Store
import mycelium.state.ago
import mycelium.state.changedByWord
import mycelium.state.envNameProblem
import mycelium.state.envNotice
import mycelium.state.envShownValue
import mycelium.state.envUsedAt
import mycelium.state.envValueProblem

private sealed interface EnvEditing {
    data object Adding : EnvEditing

    data class Changing(val env: AppEnv) : EnvEditing
}

@Composable
internal fun AppEnvironmentView(store: Store, app: AppDetail, act: (suspend () -> Unit) -> Unit) {
    val colors = MyTheme.colors
    var envs by remember(app.uuid) { mutableStateOf<List<AppEnv>?>(null) }
    var failure by remember(app.uuid) { mutableStateOf<String?>(null) }
    var revision by remember(app.uuid) { mutableStateOf(0) }
    var revealed by remember(app.uuid) { mutableStateOf(setOf<String>()) }
    var editing by remember { mutableStateOf<EnvEditing?>(null) }

    LaunchedEffect(app.uuid, revision, app.latestDeployment?.uuid) {
        failure = attempt { envs = store.api.appEnvs(app.uuid) }
    }

    val loaded = envs
    val notice = loaded?.let(::envNotice)

    Section("Variables") {
        notice?.let { message ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                Icon(MyIcon.Alert, colors.statusAttention, Tokens.Size.IconSm)
                Label(message, Modifier.weight(1f), color = colors.text)
                MyButton({ act { store.api.redeployApp(app.uuid) } }, text = "Redeploy", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }
        }

        Row { MyButton({ editing = EnvEditing.Adding }, text = "Add variable", icon = MyIcon.Plus, variant = ButtonVariant.Outline) }

        when {
            failure != null -> Label(failure.orEmpty(), color = colors.statusFailed)
            loaded == null -> Label("Loading the variables.", color = colors.textMuted)
            loaded.isEmpty() -> Label("This app has no variables.", color = colors.textMuted)
            else -> Table(
                listOf(
                    Col("Name", 120.dp, weight = 1f),
                    Col("Value", 140.dp),
                    Col("Used at", 96.dp, priority = 1),
                    Col("Last changed", 120.dp, priority = 2),
                    ActionColumn.copy(min = 72.dp, preferred = 72.dp),
                ),
                loaded.size,
                { row, column ->
                    val env = loaded[row]

                    when (column) {
                        0 -> TableCell.Text(env.key, Tokens.Type.Code)
                        1 -> TableCell.Slot(measureAs = "•••••••••• Show", extra = 32.dp) {
                            val shown = env.key in revealed

                            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                Label(envShownValue(env, shown), style = Tokens.Type.Code, maxLines = 1)

                                if (env.secret) {
                                    Tag("Secret")
                                } else if (env.value != null) {
                                    MyButton({ revealed = if (shown) revealed - env.key else revealed + env.key }, text = if (shown) "Hide" else "Show", description = "${if (shown) "Hide" else "Show"} the value of ${env.key}", size = ButtonSize.Sm, muted = true)
                                }
                            }
                        }
                        2 -> TableCell.Text(envUsedAt(env), color = colors.textMuted)
                        3 -> TableCell.Text(listOfNotNull(changedByWord(env.changedBy), env.changedAt?.let(::ago)).joinToString(", "), color = colors.textMuted)
                        else -> TableCell.Slot(measureAs = "Change") {
                            MyButton({ editing = EnvEditing.Changing(env) }, text = "Change", description = "Change ${env.key}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                        }
                    }
                },
                lazy = false,
                rowKey = { loaded[it].key },
                modifier = Modifier.fillMaxWidth(),
            )
        }

        Label("Secret values are never shown, here, to agents or in logs. To change one, enter the whole new value.", style = Tokens.Type.Meta, color = colors.textMuted)
    }

    editing?.let { target ->
        EnvDialog(store, app.uuid, (target as? EnvEditing.Changing)?.env, loaded.orEmpty(), { editing = null }) {
            editing = null
            revision += 1
        }
    }
}

@Composable
internal fun EnvDialog(store: Store, appUuid: String, env: AppEnv?, existing: List<AppEnv>, onDismiss: () -> Unit, onDone: () -> Unit) {
    val colors = MyTheme.colors
    val runner = rememberAction()
    var key by remember { mutableStateOf(env?.key.orEmpty()) }
    var value by remember { mutableStateOf("") }
    var secret by remember { mutableStateOf(env?.secret ?: false) }
    var buildTime by remember { mutableStateOf(env?.buildTime ?: false) }
    var keyProblem by remember { mutableStateOf<String?>(null) }
    var valueProblem by remember { mutableStateOf<String?>(null) }
    val name = key.trim().ifEmpty { "the variable" }

    fun save() {
        keyProblem = if (env == null) envNameProblem(key, existing) else null
        valueProblem = envValueProblem(name, value)

        if (keyProblem != null || valueProblem != null) {
            return
        }

        runner {
            store.api.putAppEnv(appUuid, key.trim(), PutAppEnvRequest(value, secret, buildTime))
            onDone()
        }
    }

    fun delete() {
        runner {
            store.api.deleteAppEnv(appUuid, key.trim())
            onDone()
        }
    }

    Dialog(onDismiss) {
        Label(if (env == null) "Add variable" else "Change ${env.key}", Modifier.semantics { heading() }, Tokens.Type.Title)

        if (env == null) {
            TextInput(key, { key = it; keyProblem = null }, label = "Name", mono = true, error = keyProblem)
        }

        TextInput(
            value,
            { value = it; valueProblem = null },
            label = "Value",
            placeholder = "Enter the new value",
            mono = true,
            masked = secret,
            error = valueProblem,
            hint = if (env == null) "Whatever you enter is stored as the whole value." else "The current value is never shown. Whatever you enter replaces it whole.",
            onSubmit = ::save,
        )

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
            Checkbox(secret, "Secret", { secret = !secret }, checkedState = "On", uncheckedState = "Off")
            Column(Modifier.padding(top = Tokens.Space.S2)) {
                Label("Secret", style = Tokens.Type.UiStrong)
                Label("Agents, logs and this page never see a secret's value.", style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
            Checkbox(buildTime, "Available at build time", { buildTime = !buildTime }, checkedState = "On", uncheckedState = "Off")
            Column(Modifier.padding(top = Tokens.Space.S2)) {
                Label("Available at build time", style = Tokens.Type.UiStrong)
                Label("Applies on the next deploy.", style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            if (env != null) {
                MyButton(::delete, text = "Delete variable", variant = ButtonVariant.Danger)
            }

            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}
