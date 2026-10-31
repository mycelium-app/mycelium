package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import mycelium.api.ApiException
import mycelium.api.ControlPlaneUnreachable
import mycelium.api.AppsCredentialRequest
import mycelium.api.ChatToolServer
import mycelium.api.CoolifyTested
import mycelium.api.CreateChatToolServerRequest
import mycelium.api.TandoorTested
import mycelium.state.Choice
import mycelium.state.CoolifyKind
import mycelium.state.Store
import mycelium.state.TandoorKind

private const val AppsNoExpiry = "none"
private const val AppsCustomExpiry = "custom"
private const val DateProblem = "Enter the expiry date as year-month-day, for example 2027-01-24."
private const val DaysInYear = 365
private const val DateLength = 10

private val AppsExpiryChoices = ExpiryPresets.map { Choice(it.toString(), "$it days") } + listOf(Choice(AppsCustomExpiry, "Custom date"), Choice(AppsNoExpiry, "No expiration"))

private val CoolifySteps = listOf("Address", "Credential", "Test", "Name")
private val TandoorSteps = listOf("Address and token", "Test", "Name")

private sealed interface AppsTest {
    data object Running : AppsTest

    data class Failed(val message: String, val backTo: Int) : AppsTest

    data class Coolify(val result: CoolifyTested) : AppsTest

    data class Tandoor(val result: TandoorTested) : AppsTest
}

internal fun tandoorSummary(result: TandoorTested): String = "Connected to the ${result.space} space as ${result.user}. Found ${result.recipes} ${if (result.recipes == 1) "recipe" else "recipes"}."

internal fun coolifySummary(result: CoolifyTested): String = "Connected to ${result.host}."

@Composable
internal fun AppsSetupFlow(
    store: Store,
    kind: String,
    knownAddress: String?,
    takenNames: Set<String>,
    onAdded: (ChatToolServer) -> Unit,
    onFinished: (ChatToolServer) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = MyTheme.colors
    val coolify = kind == CoolifyKind
    val product = if (coolify) "Coolify" else "Tandoor"
    val steps = if (coolify) CoolifySteps else TandoorSteps
    val tokenStep = if (coolify) "Credential" else "Address and token"
    var step by remember { mutableStateOf(0) }
    var address by remember { mutableStateOf(knownAddress.orEmpty()) }
    var token by remember { mutableStateOf("") }
    var expiry by remember { mutableStateOf(if (coolify) AppsNoExpiry else AppsCustomExpiry) }
    var customDate by remember { mutableStateOf(if (coolify) "" else expiryIn(DaysInYear).take(DateLength)) }
    var name by remember { mutableStateOf("") }
    var addressProblem by remember { mutableStateOf<String?>(null) }
    var tokenProblem by remember { mutableStateOf<String?>(null) }
    var dateProblem by remember { mutableStateOf<String?>(null) }
    var test by remember { mutableStateOf<AppsTest?>(null) }
    var attempts by remember { mutableStateOf(0) }
    var confirmingClose by remember { mutableStateOf(false) }
    val runner = rememberAction()
    val title = steps[step]
    val typed = address.isNotBlank() && knownAddress == null || token.isNotBlank() || name.isNotBlank()

    fun chosenExpiry(): String? = when (expiry) {
        AppsNoExpiry -> null
        AppsCustomExpiry -> expiryOn(customDate)
        else -> expiryIn(expiry.toInt())
    }

    fun expiryNote(): String? = chosenExpiry()?.let(::expiryDate)?.let { "Expires $it." }

    fun uniqueName(base: String): String {
        var candidate = base
        var number = 2

        while (integrationSlug(candidate) in takenNames) {
            candidate = "$base $number"
            number += 1
        }

        return candidate
    }

    fun addressFailure(): String? = when {
        address.trim().startsWith("http://") || address.trim().startsWith("https://") -> null
        coolify -> "Enter the address you open Coolify at, like https://coolify.example.com"
        else -> "Enter the server address, like https://recipes.example.com"
    }

    fun tokenFailure(): String? = if (token.isBlank()) "Enter your $product API token." else null

    fun dateFailure(): String? = if (expiry == AppsCustomExpiry && chosenExpiry() == null) DateProblem else null

    fun nameFailure(): String? = when {
        name.isBlank() -> "Enter a name for this account. It becomes the prefix of its tools."
        integrationSlug(name) in takenNames -> "Enter a different name. Another integration already has this one."
        else -> null
    }

    fun startTest() {
        name = name.ifBlank { uniqueName(product) }
        test = null
        step = steps.indexOf("Test")
    }

    fun next() {
        runner.failure = null

        when (title) {
            "Address" -> {
                addressProblem = addressFailure()

                if (addressProblem == null) {
                    step += 1
                }
            }

            "Credential", "Address and token" -> {
                addressProblem = if (coolify) null else addressFailure()
                tokenProblem = tokenFailure()
                dateProblem = dateFailure()

                if (addressProblem == null && tokenProblem == null && dateProblem == null) {
                    startTest()
                }
            }

            "Test" -> step += 1
        }
    }

    fun back() {
        runner.failure = null
        test = null
        step = (step - 1).coerceAtLeast(0)
    }

    fun connect() {
        val problem = nameFailure()

        if (problem != null) {
            runner.failure = problem

            return
        }

        runner {
            val added = store.api.createChatToolServer(CreateChatToolServerRequest(name.trim(), address.trim(), token.trim(), kind = kind, tokenExpiresAt = chosenExpiry()))
            onAdded(added)
            onFinished(added)
        }
    }

    LaunchedEffect(step, attempts) {
        if (steps[step] != "Test") {
            return@LaunchedEffect
        }

        test = AppsTest.Running

        try {
            val request = AppsCredentialRequest(address.trim(), token.trim())

            if (coolify) {
                val result = store.api.testCoolify(request)
                test = AppsTest.Coolify(result)
            } else {
                test = AppsTest.Tandoor(store.api.testTandoor(request))
            }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (problem: ApiException) {
            val addressWrong = problem.code == "coolify_address" || problem.code == "tandoor_address"

            test = AppsTest.Failed(problem.message, if (addressWrong) 0 else steps.indexOf(tokenStep))
        } catch (_: Exception) {
            test = AppsTest.Failed(ControlPlaneUnreachable, steps.indexOf(tokenStep))
        }
    }

    Dialog({ if (typed) confirmingClose = true else onDismiss() }) {
        val entry = BuiltInCatalogue.first { it.key == kind }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
            IntegrationMark(Integration(entry.key, entry.name, entry.kind, entry.category, emptyList(), entry.brand), 36.dp)
            Label("Add $product", Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)
            MyButton({ if (typed) confirmingClose = true else onDismiss() }, icon = MyIcon.Close, description = "Close", size = ButtonSize.Sm, muted = true)
        }

        Row(
            Modifier.fillMaxWidth().semantics { contentDescription = "Step ${step + 1} of ${steps.size}: $title" },
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            steps.forEachIndexed { index, label ->
                if (index > 0) {
                    Icon(MyIcon.ChevronRight, colors.textFaint, Tokens.Size.IconSm)
                }

                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    if (index < step) {
                        Icon(MyIcon.Check, colors.statusDone, Tokens.Size.IconSm)
                    }

                    Label(if (index < step) label else "${index + 1} $label", style = if (index == step) Tokens.Type.UiStrong else Tokens.Type.Meta, color = if (index == step) colors.text else colors.textMuted)
                }
            }
        }

        when (title) {
            "Address" -> {
                TextInput(address, { address = it; addressProblem = null }, label = "Server address", placeholder = "https://coolify.example.com", mono = true, error = addressProblem, onSubmit = ::next)
                Label("The address you open Coolify at. Coolify Cloud is https://app.coolify.io. Agents get the apps they build here, and an Apps page appears in the sidebar above Artefacts.", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            "Credential", "Address and token" -> {
                if (!coolify) {
                    TextInput(address, { address = it; addressProblem = null }, label = "Server address", placeholder = "https://recipes.example.com", mono = true, error = addressProblem, onSubmit = ::next)
                    Label("The address you open Tandoor at.", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label(if (coolify) "Where to create it" else "Where to create the token", Modifier.semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)

                    if (coolify) {
                        Label("In Coolify, open Keys & Tokens, then API Tokens.", style = Tokens.Type.Meta, color = colors.textMuted)
                        Label("Create a token with the read, write and deploy permissions. Leave read:sensitive off, so secret values never leave Coolify.", style = Tokens.Type.Meta, color = colors.textMuted)
                        Label("On a self-hosted Coolify, turn on API access in Settings, Advanced.", style = Tokens.Type.Meta, color = colors.textMuted)
                    } else {
                        Label("In Tandoor, open Settings, then API.", style = Tokens.Type.Meta, color = colors.textMuted)
                        Label("Create a token with the scope read write. A read token is enough if agents only look things up.", style = Tokens.Type.Meta, color = colors.textMuted)
                    }
                }

                TextInput(token, { token = it; tokenProblem = null }, label = "API token", mono = true, masked = true, error = tokenProblem, hint = "Stored on the control plane, never shown to agents.", onSubmit = ::next)

                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
                    OptionMenu("Expiration", AppsExpiryChoices, expiry, { expiry = it; dateProblem = null })
                    expiryNote()?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted) }
                }

                if (expiry == AppsCustomExpiry) {
                    TextInput(customDate, { customDate = it; dateProblem = null }, label = "Expiry date", placeholder = "2027-01-24", mono = true, error = dateProblem, onSubmit = ::next)
                }

                Label(
                    if (coolify) "Set the expiry you chose in Coolify, so Integrations can warn you before the token stops working." else "Tandoor gives every token an expiry. Set the same date so Integrations can warn you before it stops working.",
                    style = Tokens.Type.Meta,
                    color = colors.textMuted,
                )
            }

            "Test" -> when (val state = test) {
                AppsTest.Running, null -> Label("Testing the connection.", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, color = colors.textMuted)

                is AppsTest.Coolify -> Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Label(coolifySummary(state.result), Modifier.semantics { liveRegion = LiveRegionMode.Polite })
                    TagGroup("Projects", state.result.projects.map { it.name })
                    TagGroup("Servers", state.result.servers.map { it.name })
                    Label("Reading apps, deployments and logs runs straight away. Creating, deploying, stopping and deleting apps, and changing their environment, ask first.", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                is AppsTest.Tandoor -> Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Label(tandoorSummary(state.result), Modifier.semantics { liveRegion = LiveRegionMode.Polite })

                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Tag("Recipes")
                        Tag("Meal plans")
                        Tag("Shopping list")
                    }

                    Label("Searching and reading run straight away. Adding recipes, planning meals and changing the shopping list ask first.", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                is AppsTest.Failed -> Column(
                    Modifier.fillMaxWidth().semantics {
                        contentDescription = "Error: ${state.message}"
                        liveRegion = LiveRegionMode.Assertive
                    },
                    verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                ) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
                        Icon(MyIcon.Failed, colors.statusFailed, Tokens.Size.IconSm)
                        Label(state.message, style = Tokens.Type.Meta, color = colors.statusFailed)
                    }

                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        val backLabel = when {
                            !coolify -> "Back to address and token"
                            state.backTo == 0 -> "Back to address"
                            else -> "Back to credential"
                        }

                        MyButton({ test = null; step = state.backTo }, text = backLabel, variant = ButtonVariant.Outline)
                        MyButton({ attempts += 1 }, text = "Try again", variant = ButtonVariant.Outline)
                    }
                }
            }

            "Name" -> {
                TextInput(name, { name = it }, label = "Name", error = runner.failure, onSubmit = ::connect)
                Label("Agents see its tools as ${integrationSlug(name).ifEmpty { "name" }}__tool_name. The name must be unique.", style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        val running = title == "Test" && (test == null || test is AppsTest.Running || test is AppsTest.Failed)

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            if (step > 0) {
                MyButton(::back, text = "Back", muted = true)
            } else {
                MyButton({ if (typed) confirmingClose = true else onDismiss() }, text = "Cancel", muted = true)
            }

            if (!running) {
                MyButton(if (title == "Name") ::connect else ::next, text = if (title == "Name") "Add integration" else "Continue", variant = ButtonVariant.Outline)
            }
        }
    }

    if (confirmingClose) {
        ConfirmDialog("Discard this setup?", "What you typed is not saved.", "Discard", { confirmingClose = false }) {
            confirmingClose = false
            onDismiss()
        }
    }
}

@Composable
private fun TagGroup(title: String, names: List<String>) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Label(title, style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            for (name in names) {
                Tag(name)
            }
        }
    }
}
