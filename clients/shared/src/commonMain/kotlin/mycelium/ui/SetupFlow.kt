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
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.ApiException
import mycelium.api.CalendarAccess
import mycelium.api.ChatToolServer
import mycelium.api.CreateChatToolServerRequest
import mycelium.api.ProbeChatToolServerRequest
import mycelium.api.StartChatToolServerOAuthRequest
import mycelium.api.TestCalendarsRequest
import mycelium.api.TestChatToolServerRequest
import mycelium.state.Choice
import mycelium.state.Store

private const val Token = "Token"
private const val OAuth = "Sign in with OAuth"
private const val NoExpiry = "none"
private const val CustomExpiry = "custom"

private val McpSteps = listOf("Address", "Credential", "Test", "Name")
private val LastFmSteps = listOf("API key", "Test", "Sign in")
private val FormSteps = listOf("Address", "Credential", "Name")

private const val Tls = "TLS"
private const val StartTls = "STARTTLS"
private const val UserSession = "User session"
private const val Bot = "Bot"
private const val StoatAddress = "https://stoat.chat/api"

private val ExpiryChoices = ExpiryPresets.map { Choice(it.toString(), "$it days") } + listOf(Choice(CustomExpiry, "Custom date"), Choice(NoExpiry, "No expiration"))

private sealed interface TestState {
    data object Running : TestState

    data class Failed(val message: String) : TestState

    data class Passed(val summary: String, val tools: List<mycelium.api.TestedTool> = emptyList()) : TestState
}

fun defaultIntegrationName(address: String, fallback: String): String {
    val host = address.trim().removePrefix("https://").removePrefix("http://").substringBefore('/').substringBefore(':').removePrefix("www.")

    return host.ifBlank { fallback }
}

fun integrationSlug(name: String): String = name.trim().lowercase().map { if (it.isLetterOrDigit()) it else '_' }.joinToString("").trim('_')

private fun toolSummary(tools: List<mycelium.api.TestedTool>): String =
    "Connected. The server offers ${tools.size} ${if (tools.size == 1) "tool" else "tools"}."

private fun toolBreakdown(tools: List<mycelium.api.TestedTool>): String {
    val reads = tools.count { it.kind == "read" }
    val writes = tools.size - reads

    return "$reads read ${if (reads == 1) "tool" else "tools"}, $writes write ${if (writes == 1) "tool" else "tools"}. Write tools will ask first."
}

private const val ShownTools = 6

@Composable
fun SetupFlow(
    store: Store,
    kind: String,
    knownAddress: String?,
    takenNames: Set<String>,
    onAdded: (ChatToolServer) -> Unit,
    onFinished: (ChatToolServer) -> Unit,
    onSigningIn: () -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val lastFm = kind == "lastfm"
    val caldav = kind == "caldav"
    val form = kind == "zulip" || kind == "email" || kind == "stoat"
    val mcp = !lastFm && !caldav && !form
    val steps = when {
        lastFm -> LastFmSteps
        form -> FormSteps
        else -> McpSteps
    }
    val product = when (kind) {
        "outline" -> "Outline"
        "litellm" -> "LiteLLM"
        "lastfm" -> "Last.fm"
        "caldav" -> "Calendar"
        "zulip" -> "Zulip"
        "email" -> "Email"
        "stoat" -> "Stoat"
        else -> "MCP server"
    }
    var step by remember { mutableStateOf(if (knownAddress != null) 1 else 0) }
    var address by remember { mutableStateOf(knownAddress.orEmpty()) }
    var oauthOffered by remember { mutableStateOf(false) }
    var method by remember { mutableStateOf(Token) }
    var token by remember { mutableStateOf("") }
    var expiry by remember { mutableStateOf(NoExpiry) }
    var customDate by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var username by remember { mutableStateOf("") }
    var apiKey by remember { mutableStateOf("") }
    var secret by remember { mutableStateOf("") }
    var imapHost by remember { mutableStateOf("") }
    var imapPort by remember { mutableStateOf("") }
    var imapSecurity by remember { mutableStateOf(Tls) }
    var smtpHost by remember { mutableStateOf("") }
    var smtpPort by remember { mutableStateOf("") }
    var smtpSecurity by remember { mutableStateOf(Tls) }
    var sender by remember { mutableStateOf("") }
    var tokenType by remember { mutableStateOf(UserSession) }
    var testState by remember { mutableStateOf<TestState?>(null) }
    var calendars by remember { mutableStateOf<List<CalendarAccess>>(emptyList()) }
    var failedStep by remember { mutableStateOf(0) }
    var tested by remember { mutableStateOf(false) }
    var created by remember { mutableStateOf<ChatToolServer?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var confirmingClose by remember { mutableStateOf(false) }
    var attempts by remember { mutableStateOf(0) }
    val attempting = rememberAttempt { failure = it }
    val typed = address.isNotBlank() && knownAddress == null || token.isNotBlank() || name.isNotBlank() || username.isNotBlank() || apiKey.isNotBlank() || secret.isNotBlank() || imapHost.isNotBlank() || smtpHost.isNotBlank()
    val title = steps[step]

    fun chosenExpiry(): String? = when (expiry) {
        NoExpiry -> null
        CustomExpiry -> expiryOn(customDate)
        else -> expiryIn(expiry.toInt())
    }

    fun expiresAt(): String? = when {
        lastFm || form || method == OAuth -> null
        else -> chosenExpiry()
    }

    fun expiryNote(): String? = when (expiry) {
        NoExpiry -> null
        CustomExpiry -> expiryOn(customDate)?.let(::expiryDate)?.let { "Expires $it." }
        else -> expiryDate(expiryIn(expiry.toInt()))?.let { "Expires $it." }
    }

    fun uniqueName(base: String): String {
        var candidate = base
        var number = 2

        while (integrationSlug(candidate) in takenNames) {
            candidate = "$base $number"
            number += 1
        }

        return candidate
    }

    fun nameFailure(): String? = when {
        name.isBlank() -> "Enter a name for this account. It becomes the prefix of its tools."
        integrationSlug(name) in takenNames -> "Enter a different name. Another integration already has this one."
        else -> null
    }

    fun lastFmRequest() = CreateChatToolServerRequest(
        name = name.trim().ifEmpty { "Last.fm" },
        url = "",
        bearerToken = secret.trim().ifEmpty { null },
        kind = "lastfm",
        account = username.trim().ifEmpty { null },
        settings = JsonObject(mapOf("api_key" to JsonPrimitive(apiKey.trim()))),
    )

    fun emailSettings(): JsonObject {
        val fields = mutableMapOf<String, kotlinx.serialization.json.JsonElement>(
            "imap_host" to JsonPrimitive(imapHost.trim()),
            "imap_tls" to JsonPrimitive(if (imapSecurity == Tls) "tls" else "starttls"),
            "smtp_host" to JsonPrimitive(smtpHost.trim()),
            "smtp_tls" to JsonPrimitive(if (smtpSecurity == Tls) "tls" else "starttls"),
        )

        imapPort.trim().toIntOrNull()?.let { fields["imap_port"] = JsonPrimitive(it) }
        smtpPort.trim().toIntOrNull()?.let { fields["smtp_port"] = JsonPrimitive(it) }

        if (sender.isNotBlank()) {
            fields["from"] = JsonPrimitive(sender.trim())
        }

        return JsonObject(fields)
    }

    fun formRequest(): CreateChatToolServerRequest = when (kind) {
        "zulip" -> CreateChatToolServerRequest(name.trim(), address.trim(), token.trim().ifEmpty { null }, kind = "zulip", account = username.trim().ifEmpty { null })
        "email" -> CreateChatToolServerRequest(name.trim(), "", token.trim().ifEmpty { null }, kind = "email", account = username.trim().ifEmpty { null }, settings = emailSettings())
        else -> CreateChatToolServerRequest(name.trim(), address.trim().ifEmpty { StoatAddress }, token.trim().ifEmpty { null }, kind = "stoat", settings = JsonObject(mapOf("token_kind" to JsonPrimitive(if (tokenType == Bot) "bot" else "user"))))
    }

    fun nextFormStep() {
        when (step) {
            0 -> {
                val problem = when {
                    kind == "zulip" && !address.trim().startsWith("http") -> "Enter the Zulip server address, starting with https://, for example https://example.zulipchat.com."
                    kind == "email" && (imapHost.isBlank() || smtpHost.isBlank()) -> "Enter the IMAP and SMTP servers, for example imap.example.com and smtp.example.com."
                    kind == "email" && imapPort.isNotBlank() && imapPort.trim().toIntOrNull() == null -> "Enter the IMAP port as a number, for example 993."
                    kind == "email" && smtpPort.isNotBlank() && smtpPort.trim().toIntOrNull() == null -> "Enter the SMTP port as a number, for example 465."
                    else -> null
                }

                if (problem != null) {
                    failure = problem

                    return
                }

                name = name.ifBlank { product }
                step = 1
            }

            1 -> {
                val problem = when {
                    kind != "stoat" && username.isBlank() -> if (kind == "zulip") "Enter your Zulip email address." else "Enter your email username."
                    token.isBlank() -> "Enter the credential for this account."
                    else -> null
                }

                if (problem != null) {
                    failure = problem

                    return
                }

                if (integrationSlug(name) in takenNames) {
                    name = "$product ${username.trim().ifEmpty { defaultIntegrationName(address, product) }}".trim()
                }

                step = 2
            }
        }
    }

    fun next() {
        failure = null

        when {
            form -> nextFormStep()

            lastFm && step == 0 -> {
                name = name.ifBlank { "Last.fm" }.let { if (integrationSlug(it) in takenNames) "$it ${username.trim()}".trim() else it }
                testState = null
                step = 1
            }

            caldav && step == 0 -> {
                if (!address.trim().startsWith("http")) {
                    failure = "Enter the server's address, starting with http:// or https://, for example https://cal.example.com."

                    return
                }

                name = name.ifBlank { product }
                step = 1
            }

            caldav && step == 1 -> {
                when {
                    username.isBlank() -> failure = "Enter your calendar username."
                    secret.isBlank() -> failure = "Enter an app password for your calendar account."
                    expiry == CustomExpiry && chosenExpiry() == null -> failure = "Enter the expiry date as year-month-day, for example 2027-01-24."
                    else -> {
                        testState = null
                        step = 2
                    }
                }
            }

            caldav && step == 2 -> step = 3

            mcp && step == 0 -> attempting {
                if (!address.trim().startsWith("http")) {
                    failure = "Enter the server's address, starting with http:// or https://, for example https://example.com/mcp."

                    return@attempting
                }

                oauthOffered = store.api.probeChatToolServer(ProbeChatToolServerRequest(address.trim())).oauth
                method = if (oauthOffered) OAuth else Token
                name = name.ifBlank { defaultIntegrationName(address, product) }
                step = 1
            }

            mcp && step == 1 && method == OAuth -> attempting {
                if (nameFailure() != null || name.isBlank()) {
                    name = uniqueName(defaultIntegrationName(address, product))
                }

                val started = store.api.startChatToolServerOAuth(StartChatToolServerOAuthRequest(name.trim(), address.trim(), if (kind == "outline") "outline" else null))
                platform?.openUrl(started.authorizeUrl)
                onSigningIn()
            }

            mcp && step == 1 -> {
                if (name.isBlank()) {
                    name = uniqueName(defaultIntegrationName(address, product))
                }

                if (expiry == CustomExpiry && chosenExpiry() == null) {
                    failure = "Enter the expiry date as year-month-day, for example 2027-01-24."

                    return
                }

                testState = null
                step = 2
            }

            mcp && step == 2 -> step = 3
        }
    }

    fun back() {
        failure = null
        testState = null
        step = (step - 1).coerceAtLeast(if (knownAddress != null) 1 else 0)
    }

    fun backTo(target: Int) {
        failure = null
        testState = null
        step = target.coerceAtLeast(if (knownAddress != null) 1 else 0)
    }

    fun connect() {
        val problem = nameFailure()

        if (problem != null) {
            failure = problem

            return
        }

        attempting {
            val credential = if (caldav) secret else token
            val request = if (form) formRequest() else CreateChatToolServerRequest(name.trim(), address.trim(), credential.trim().ifEmpty { null }, kind = kind.takeIf { it != "mcp" }, account = username.trim().takeIf { caldav }, tokenExpiresAt = expiresAt())
            val added = store.api.createChatToolServer(request)
            onAdded(added)
            onFinished(added)
        }
    }

    LaunchedEffect(step, attempts) {
        if (steps[step] != "Test") {
            return@LaunchedEffect
        }

        testState = TestState.Running
        tested = false

        val problem = attempt {
            if (caldav) {
                try {
                    val result = store.api.testCalendars(TestCalendarsRequest(address.trim(), username.trim(), secret))
                    calendars = result.calendars
                    testState = TestState.Passed("Connected as ${result.username}. Found ${result.calendars.size} ${if (result.calendars.size == 1) "calendar" else "calendars"}.")
                } catch (problem: ApiException) {
                    failedStep = if (problem.code == "caldav_credential") 1 else 0
                    throw problem
                }
            } else if (lastFm) {
                val added = store.api.createChatToolServer(lastFmRequest())
                created = added
                onAdded(added)
                testState = TestState.Passed("Connected. ${username.trim()} is ready to read. Sign in to love tracks and scrobble.")
            } else {
                val result = store.api.testChatToolServer(TestChatToolServerRequest(address.trim(), token.trim().ifEmpty { null }))
                testState = TestState.Passed(toolSummary(result.tools), result.tools)
            }

            tested = true
        }

        if (problem != null) {
            testState = TestState.Failed(problem)
        }
    }

    Dialog({ if (typed && created == null) confirmingClose = true else onDismiss() }) {
        val entry = BuiltInCatalogue.find { it.key == kind } ?: AnyMcpEntry

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
            IntegrationMark(Integration(entry.key, entry.name, entry.kind, entry.category, emptyList(), entry.brand), 36.dp)
            Label(if (caldav) "Add a calendar" else if (kind == "mcp") "Add an MCP server" else "Add $product", Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)
            MyButton({ if (typed && created == null) confirmingClose = true else onDismiss() }, icon = MyIcon.Close, description = "Close", size = ButtonSize.Sm, muted = true)
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
            "Address" -> if (kind == "zulip") {
                Label("Chats and agents can read and search your Zulip messages and, with your approval, send them.", style = Tokens.Type.Meta, color = colors.textMuted)
                TextInput(address, { address = it }, label = "Zulip server address", placeholder = "https://example.zulipchat.com", mono = true, onSubmit = ::next)
            } else if (kind == "email") {
                Label("Chats and agents can read and search your mail and, with your approval, send it.", style = Tokens.Type.Meta, color = colors.textMuted)
                TextInput(imapHost, { imapHost = it }, label = "IMAP server", placeholder = "imap.example.com", mono = true)
                TextInput(imapPort, { imapPort = it }, label = "IMAP port, if not the usual one", placeholder = if (imapSecurity == Tls) "993" else "143", mono = true)
                Segmented(listOf(Tls, StartTls), imapSecurity, { imapSecurity = it }, "IMAP security")
                TextInput(smtpHost, { smtpHost = it }, label = "SMTP server", placeholder = "smtp.example.com", mono = true)
                TextInput(smtpPort, { smtpPort = it }, label = "SMTP port, if not the usual one", placeholder = if (smtpSecurity == Tls) "465" else "587", mono = true)
                Segmented(listOf(Tls, StartTls), smtpSecurity, { smtpSecurity = it }, "SMTP security")
            } else if (kind == "stoat") {
                Label("Chats and agents can read and search Stoat messages and, with your approval, send them.", style = Tokens.Type.Meta, color = colors.textMuted)
                TextInput(address, { address = it }, label = "Stoat API address", placeholder = StoatAddress, mono = true, onSubmit = ::next)
            } else if (caldav) {
                TextInput(address, { address = it }, label = "Server address", placeholder = "https://cal.example.com", mono = true, onSubmit = ::next)
                Label("The base address is enough. Mycelium finds your calendars from it.", style = Tokens.Type.Meta, color = colors.textMuted)
                Label("Works with any CalDAV server, such as Nextcloud, Radicale, Fastmail or iCloud.", style = Tokens.Type.Meta, color = colors.textMuted)
            } else {
                TextInput(address, { address = it }, label = if (kind == "mcp") "Server address" else "$product address", placeholder = "https://example.com/mcp", mono = true, onSubmit = ::next)
                Label(if (kind == "mcp") "The address the server's documentation gives for MCP over HTTP." else "Mycelium checks whether the server offers sign-in before asking for a credential.", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            "Credential" -> if (form) {
                if (kind == "zulip") {
                    TextInput(username, { username = it }, label = "Zulip email address", mono = true)
                    TextInput(token, { token = it }, label = "Zulip API key", mono = true, masked = true, onSubmit = ::next)
                    Label("Find your API key in Zulip under Personal settings, Account and privacy.", style = Tokens.Type.Meta, color = colors.textMuted)
                } else if (kind == "email") {
                    TextInput(username, { username = it }, label = "Email username", placeholder = "ada@example.com", mono = true)
                    TextInput(token, { token = it }, label = "Email password or app password", mono = true, masked = true)
                    TextInput(sender, { sender = it }, label = "Send from address, if it differs from the username", mono = true, onSubmit = ::next)
                    Label("Your provider may need an app password instead of your usual one.", style = Tokens.Type.Meta, color = colors.textMuted)
                } else {
                    Segmented(listOf(UserSession, Bot), tokenType, { tokenType = it }, "Stoat token type")
                    TextInput(token, { token = it }, label = "Stoat token", mono = true, masked = true, onSubmit = ::next)
                    Label("Use a bot token for a bot, or your session token to act as yourself.", style = Tokens.Type.Meta, color = colors.textMuted)
                }
            } else {
                if (caldav) {
                    Label("Sign in to ${defaultIntegrationName(address, address)}.")
                    TextInput(username, { username = it }, label = "Username")
                    TextInput(secret, { secret = it }, label = "App password", mono = true, masked = true, onSubmit = ::next)
                    Label("Create one in your calendar provider's security settings. Stored on the control plane, never shown to agents.", style = Tokens.Type.Meta, color = colors.textMuted)
                } else {
                    Label("${defaultIntegrationName(address, address)} asks for a credential.")

                    if (oauthOffered) {
                        Label("How to sign in", style = Tokens.Type.Meta, color = colors.textMuted)
                        Segmented(listOf(Token, OAuth), method, { method = it }, "How to sign in")
                    }
                }

                if (method == Token) {
                    if (!caldav) {
                        TextInput(token, { token = it }, label = if (kind == "outline") "Outline API key" else "Token", mono = true, masked = true, onSubmit = ::next)
                        Label("Sent as a bearer token. Stored on the control plane, never shown to agents.", style = Tokens.Type.Meta, color = colors.textMuted)
                    }

                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
                        OptionMenu("Expiration", ExpiryChoices, expiry, { expiry = it })
                        expiryNote()?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted) }
                    }

                    if (expiry == CustomExpiry) {
                        TextInput(customDate, { customDate = it }, label = "Expiry date", placeholder = "2027-01-24", mono = true, onSubmit = ::next)
                    }

                    Label(if (caldav) "Set this if your provider gave the app password an end date, so Integrations can warn you before it stops working." else "Set what the server gave the token, so Integrations can warn you before it stops working.", style = Tokens.Type.Meta, color = colors.textMuted)
                } else {
                    Label("Mycelium opens the server's sign-in in your browser and connects the account when you finish.", style = Tokens.Type.Meta, color = colors.textMuted)
                }
            }

            "API key" -> {
                Label("Create an API account at last.fm/api/account/create.", style = Tokens.Type.Meta, color = colors.textMuted)
                TextInput(username, { username = it }, label = "Last.fm username", mono = true)
                TextInput(apiKey, { apiKey = it }, label = "Last.fm API key", mono = true)
                TextInput(secret, { secret = it }, label = "Last.fm shared secret", mono = true, masked = true, onSubmit = ::next)
            }

            "Test" -> {
                when (val state = testState) {
                    TestState.Running, null -> Label("Testing the connection.", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, color = colors.textMuted)
                    is TestState.Passed -> Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        Label(state.summary, Modifier.semantics { liveRegion = LiveRegionMode.Polite })

                        if (state.tools.isNotEmpty()) {
                            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                                for (tool in state.tools.take(ShownTools)) {
                                    Tag(tool.name)
                                }

                                if (state.tools.size > ShownTools) {
                                    Tag("${state.tools.size - ShownTools} more")
                                }
                            }

                            Label(toolBreakdown(state.tools), style = Tokens.Type.Meta, color = colors.textMuted)
                        }

                        if (caldav) {
                            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                                for (calendar in calendars) {
                                    Tag(if (calendar.readOnly) "${calendar.name}, read only" else calendar.name)
                                }
                            }

                            Label("Listing and searching events run straight away. Creating, changing and deleting events ask first.", style = Tokens.Type.Meta, color = colors.textMuted)
                        }
                    }
                    is TestState.Failed -> Column(
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
                            if (caldav) {
                                MyButton({ backTo(failedStep) }, text = if (failedStep == 1) "Back to credential" else "Back to address", variant = ButtonVariant.Outline)
                            } else {
                                MyButton(::back, text = if (lastFm) "Back to API key" else "Back to address", variant = ButtonVariant.Outline)
                            }

                            MyButton({ attempts += 1 }, text = "Try again", variant = ButtonVariant.Outline)
                        }
                    }
                }
            }

            "Name" -> {
                TextInput(name, { name = it }, label = "Name", error = failure, onSubmit = ::connect)
                Label("Agents see its tools as ${integrationSlug(name).ifEmpty { "name" }}__tool_name. The name must be unique.", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            "Sign in" -> {
                Label("Signing in is optional. It lets agents love tracks and scrobble, with your approval each time.", color = colors.textMuted)
                MyButton(
                    { created?.let { account -> store.act { platform?.openUrl(store.api.startLastFmSignIn(account.id).authorizeUrl) } } },
                    text = "Sign in to Last.fm",
                    variant = ButtonVariant.Outline,
                )
            }
        }

        if (title != "Name") {
            failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }
        }

        val running = title == "Test" && testState !is TestState.Passed

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            if (title == "Sign in") {
                MyButton(onDismiss, text = "Done", variant = ButtonVariant.Outline)
            } else {
                if (step > (if (knownAddress != null) 1 else 0) && !(lastFm && step == 1)) {
                    MyButton(::back, text = "Back", muted = true)
                } else {
                    MyButton({ if (typed && created == null) confirmingClose = true else onDismiss() }, text = "Cancel", muted = true)
                }

                if (!running) {
                    MyButton(
                        if (title == "Name") ::connect else ::next,
                        text = when {
                            title == "Name" -> "Add integration"
                            title == "Credential" && method == OAuth -> "Sign in"
                            else -> "Continue"
                        },
                        variant = ButtonVariant.Outline,
                    )
                }
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
