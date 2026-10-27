package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonPrimitive
import mycelium.api.ApiException
import mycelium.api.Forge
import mycelium.api.ChatToolServer
import mycelium.api.CreateChatToolServerRequest
import mycelium.api.PatchChatToolServerRequest
import mycelium.api.StartChatToolServerOAuthRequest
import mycelium.state.Store

private const val McpType = "MCP"
private const val LiteLlmType = "LiteLLM"
private const val OutlineType = "Outline"
private const val ZulipType = "Zulip"
private const val EmailType = "Email"
private const val StoatType = "Stoat"
private const val LastFmType = "Last.fm"

private const val StoatAddress = "https://stoat.chat/api"
private const val Tls = "TLS"
private const val StartTls = "STARTTLS"
private const val UserSession = "User session"
private const val Bot = "Bot"

private sealed interface IntegrationsView {
    data object Home : IntegrationsView

    data object Catalogue : IntegrationsView

    data class Setup(val kind: String, val returnTo: IntegrationsView) : IntegrationsView

    data class Page(val key: String) : IntegrationsView

    data class Activity(val key: String?) : IntegrationsView
}

private fun formKind(entry: CatalogueEntry): String = when (entry.kind) {
    "email" -> EmailType
    "zulip" -> ZulipType
    "stoat" -> StoatType
    "outline" -> OutlineType
    "lastfm" -> LastFmType
    "litellm" -> LiteLlmType
    else -> McpType
}

@Composable
fun ChatToolsSettings(store: Store) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    var servers by remember { mutableStateOf<List<ChatToolServer>>(emptyList()) }
    var forges by remember { mutableStateOf<List<Forge>>(emptyList()) }
    var view by remember { mutableStateOf<IntegrationsView>(IntegrationsView.Home) }
    var name by remember { mutableStateOf("") }
    var url by remember { mutableStateOf("") }
    var token by remember { mutableStateOf("") }
    var kind by remember { mutableStateOf(McpType) }
    var account by remember { mutableStateOf("") }
    var imapHost by remember { mutableStateOf("") }
    var imapPort by remember { mutableStateOf("") }
    var imapSecurity by remember { mutableStateOf(Tls) }
    var smtpHost by remember { mutableStateOf("") }
    var smtpPort by remember { mutableStateOf("") }
    var smtpSecurity by remember { mutableStateOf(Tls) }
    var sender by remember { mutableStateOf("") }
    var tokenType by remember { mutableStateOf(UserSession) }
    var apiKey by remember { mutableStateOf("") }
    var failure by remember { mutableStateOf<String?>(null) }
    var removing by remember { mutableStateOf<Integration?>(null) }

    val live by store.state.collectAsState()
    val platform = LocalPlatform.current
    val openConversation = LocalOpenConversation.current
    var signingIn by remember { mutableStateOf(false) }

    fun resetForm() {
        (view as? IntegrationsView.Setup)?.let { view = it.returnTo }
        name = ""
        url = ""
        token = ""
        account = ""
        imapHost = ""
        imapPort = ""
        imapSecurity = Tls
        smtpHost = ""
        smtpPort = ""
        smtpSecurity = Tls
        sender = ""
        tokenType = UserSession
        apiKey = ""
        signingIn = false
        failure = null
    }

    LaunchedEffect(live.integrationsRevision) {
        val known = servers.map { it.id }.toSet()

        store.act {
            servers = store.api.chatToolServers()

            if (signingIn) {
                resetForm()
                integrationsOf(servers).firstOrNull { integration -> integration.accounts.any { it.id !in known } }?.let { view = IntegrationsView.Page(it.key) }
            }
        }

        store.act { forges = store.api.forges() }
    }

    fun emailSettings(): JsonObject? {
        val imap = imapPort.trim()
        val smtp = smtpPort.trim()

        if (imap.isNotEmpty() && imap.toIntOrNull() == null) {
            failure = "Enter the IMAP port as a number, for example 993."
            return null
        }

        if (smtp.isNotEmpty() && smtp.toIntOrNull() == null) {
            failure = "Enter the SMTP port as a number, for example 465."
            return null
        }

        val fields = mutableMapOf<String, JsonElement>(
            "imap_host" to JsonPrimitive(imapHost.trim()),
            "imap_tls" to JsonPrimitive(if (imapSecurity == Tls) "tls" else "starttls"),
            "smtp_host" to JsonPrimitive(smtpHost.trim()),
            "smtp_tls" to JsonPrimitive(if (smtpSecurity == Tls) "tls" else "starttls"),
        )

        imap.toIntOrNull()?.let { fields["imap_port"] = JsonPrimitive(it) }
        smtp.toIntOrNull()?.let { fields["smtp_port"] = JsonPrimitive(it) }

        if (sender.isNotBlank()) {
            fields["from"] = JsonPrimitive(sender.trim())
        }

        return JsonObject(fields)
    }

    fun named(fallback: String): String {
        val typed = name.trim()
        val addingAccount = (view as? IntegrationsView.Setup)?.returnTo is IntegrationsView.Page

        return when {
            typed.isNotEmpty() -> typed
            addingAccount -> "$fallback ${account.trim().ifEmpty { url.trim().removePrefix("https://").trimEnd('/') }}".trim()
            else -> fallback
        }
    }

    fun add() {
        scope.launch {
            try {
                val request = when (kind) {
                    LiteLlmType -> CreateChatToolServerRequest(named(LiteLlmType), url.trim(), token.trim().ifEmpty { null }, kind = "litellm")
                    OutlineType -> CreateChatToolServerRequest(named(OutlineType), url.trim(), token.trim().ifEmpty { null }, kind = "outline")
                    ZulipType -> CreateChatToolServerRequest(named(ZulipType), url.trim(), token.trim().ifEmpty { null }, kind = "zulip", account = account.trim().ifEmpty { null })
                    EmailType -> CreateChatToolServerRequest(named(EmailType), "", token.trim().ifEmpty { null }, kind = "email", account = account.trim().ifEmpty { null }, settings = emailSettings() ?: return@launch)
                    StoatType -> CreateChatToolServerRequest(named(StoatType), url.trim().ifEmpty { StoatAddress }, token.trim().ifEmpty { null }, kind = "stoat", settings = JsonObject(mapOf("token_kind" to JsonPrimitive(if (tokenType == Bot) "bot" else "user"))))
                    LastFmType -> CreateChatToolServerRequest(named(LastFmType), "", token.trim().ifEmpty { null }, kind = "lastfm", account = account.trim().ifEmpty { null }, settings = JsonObject(mapOf("api_key" to JsonPrimitive(apiKey.trim()))))
                    else -> CreateChatToolServerRequest(name.trim(), url.trim(), token.trim().ifEmpty { null })
                }
                val added = store.api.createChatToolServer(request)
                servers = servers.filter { it.id != added.id } + added
                resetForm()
                view = IntegrationsView.Page(integrationsOf(servers).first { integration -> integration.accounts.any { it.id == added.id } }.key)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (problem: ApiException) {
                failure = problem.message
            } catch (_: Exception) {
                failure = "Could not reach the control plane. Check your connection and try again."
            }
        }
    }

    fun signIn() {
        scope.launch {
            try {
                val started = store.api.startChatToolServerOAuth(StartChatToolServerOAuthRequest(named(if (kind == OutlineType) OutlineType else "MCP server"), url.trim(), if (kind == OutlineType) "outline" else null))
                platform?.openUrl(started.authorizeUrl)
                signingIn = true
                failure = null
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (problem: ApiException) {
                failure = problem.message
            } catch (_: Exception) {
                failure = "Could not reach the control plane. Check your connection and try again."
            }
        }
    }

    fun signInToLastFm(target: ChatToolServer) {
        store.act {
            val started = store.api.startLastFmSignIn(target.id)
            platform?.openUrl(started.authorizeUrl)
        }
    }

    fun reauthenticate(target: ChatToolServer) {
        if (target.kind == "lastfm") {
            signInToLastFm(target)
        } else if (target.oauth) {
            store.act {
                val started = store.api.startChatToolServerOAuth(StartChatToolServerOAuthRequest(target.name, target.url, if (target.settings?.get("app")?.jsonPrimitive?.content == "outline") "outline" else null, forgeId = target.forgeId))
                platform?.openUrl(started.authorizeUrl)
            }
        }
    }

    fun addForgeServer(forge: Forge) {
        store.act {
            val started = store.api.startChatToolServerOAuth(StartChatToolServerOAuthRequest(forgeProduct(forge), mcpAddress(forge), forgeId = forge.id))
            platform?.openUrl(started.authorizeUrl)
        }
    }

    fun replace(updated: ChatToolServer) {
        servers = servers.map { if (it.id == updated.id) updated else it }
    }

    val integrations = integrationsOf(servers)
    val current = view

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        when (current) {
            IntegrationsView.Home -> IntegrationsHome(
                servers,
                IntegrationActions(
                    { view = IntegrationsView.Page(it.key) },
                    { integration ->
                        val only = integration.accounts.singleOrNull()

                        if (only != null && (only.oauth || only.kind == "lastfm")) {
                            reauthenticate(only)
                        } else {
                            view = IntegrationsView.Page(integration.key)
                        }
                    },
                    { integration ->
                        val enable = integration.group == IntegrationGroup.Disabled

                        for (account in integration.accounts) {
                            store.act { replace(store.api.patchChatToolServer(account.id, PatchChatToolServerRequest(enabled = enable))) }
                        }
                    },
                    { removing = it },
                ),
                { view = IntegrationsView.Catalogue },
                { view = IntegrationsView.Activity(null) },
            )

            IntegrationsView.Catalogue -> IntegrationsCatalogue(
                integrations,
                forges,
                { view = IntegrationsView.Home },
                { entry ->
                    kind = formKind(entry)
                    failure = null
                    view = IntegrationsView.Setup(kind, IntegrationsView.Catalogue)
                },
                { view = IntegrationsView.Page(it.key) },
                ::addForgeServer,
            )

            is IntegrationsView.Page -> {
                val integration = integrations.find { it.key == current.key }

                if (integration == null) {
                    view = IntegrationsView.Home
                } else {
                    IntegrationPage(
                        store,
                        integration,
                        PageActions(
                            { view = IntegrationsView.Home },
                            {
                                kind = formKind(entryFor(integration) ?: AnyMcpEntry)
                                failure = null
                                view = IntegrationsView.Setup(kind, current)
                            },
                            ::replace,
                            { removed -> servers = servers.filter { it.id != removed.id } },
                            ::reauthenticate,
                            { view = IntegrationsView.Activity(it.key) },
                        ),
                        { openConversation?.invoke(it) },
                    )
                }
            }

            is IntegrationsView.Activity -> IntegrationsActivity(store, integrations, integrations.find { it.key == current.key }, { view = IntegrationsView.Home }, { openConversation?.invoke(it) })

            is IntegrationsView.Setup -> {
                BackButton(if (current.returnTo is IntegrationsView.Page) "the integration" else "Add integration", ::resetForm, showText = true)

                Column(Modifier.widthIn(max = Tokens.Size.FormColumn), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                    Label(if (current.returnTo is IntegrationsView.Page) "Add an account" else "Add an integration", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                    if (kind == LiteLlmType) {
                        Label("Chats and agents can use the MCP servers your LiteLLM proxy exposes to this key. Enter the proxy's address, or its /mcp/ address for an access group.", style = Tokens.Type.Meta, color = colors.textMuted)
                        TextInput(name, { name = it }, label = "Name", placeholder = "LiteLLM")
                        TextInput(url, { url = it }, label = "LiteLLM address", placeholder = "https://litellm.example.com", mono = true)
                        TextInput(token, { token = it }, label = "LiteLLM key", mono = true, masked = true, onSubmit = ::add)
                    } else if (kind == OutlineType) {
                        Label("Chats and agents can search and read your Outline documents and, with your approval, change them. Create an API key in Outline under Settings, API & Apps.", style = Tokens.Type.Meta, color = colors.textMuted)
                        TextInput(name, { name = it }, label = "Name", placeholder = "Outline")
                        TextInput(url, { url = it }, label = "Outline address", placeholder = "https://docs.example.com", mono = true)
                        TextInput(token, { token = it }, label = "Outline API key", mono = true, masked = true, onSubmit = ::add)
                    } else if (kind == ZulipType) {
                        Label("Chats and agents can read and search your Zulip messages and, with your approval, send them. Find your API key in Zulip under Personal settings, Account and privacy.", style = Tokens.Type.Meta, color = colors.textMuted)
                        TextInput(name, { name = it }, label = "Name", placeholder = "Zulip")
                        TextInput(url, { url = it }, label = "Zulip server address", placeholder = "https://example.zulipchat.com", mono = true)
                        TextInput(account, { account = it }, label = "Zulip email address", mono = true)
                        TextInput(token, { token = it }, label = "Zulip API key", mono = true, masked = true, onSubmit = ::add)
                    } else if (kind == EmailType) {
                        Label("Chats and agents can read and search your mail and, with your approval, send it. Your provider may need an app password instead of your usual one.", style = Tokens.Type.Meta, color = colors.textMuted)
                        TextInput(name, { name = it }, label = "Name", placeholder = "Email")
                        TextInput(account, { account = it }, label = "Email username", placeholder = "ada@example.com", mono = true)
                        TextInput(token, { token = it }, label = "Email password or app password", mono = true, masked = true)
                        TextInput(imapHost, { imapHost = it }, label = "IMAP server", placeholder = "imap.example.com", mono = true)
                        TextInput(imapPort, { imapPort = it }, label = "IMAP port, if not the usual one", placeholder = if (imapSecurity == Tls) "993" else "143", mono = true)
                        Segmented(listOf(Tls, StartTls), imapSecurity, { imapSecurity = it }, "IMAP security")
                        TextInput(smtpHost, { smtpHost = it }, label = "SMTP server", placeholder = "smtp.example.com", mono = true)
                        TextInput(smtpPort, { smtpPort = it }, label = "SMTP port, if not the usual one", placeholder = if (smtpSecurity == Tls) "465" else "587", mono = true)
                        Segmented(listOf(Tls, StartTls), smtpSecurity, { smtpSecurity = it }, "SMTP security")
                        TextInput(sender, { sender = it }, label = "Send from address, if it differs from the username", mono = true, onSubmit = ::add)
                    } else if (kind == StoatType) {
                        Label("Chats and agents can read and search Stoat messages and, with your approval, send them. Use a bot token for a bot, or your session token to act as yourself.", style = Tokens.Type.Meta, color = colors.textMuted)
                        TextInput(name, { name = it }, label = "Name", placeholder = "Stoat")
                        TextInput(url, { url = it }, label = "Stoat API address", placeholder = StoatAddress, mono = true)
                        Segmented(listOf(UserSession, Bot), tokenType, { tokenType = it }, "Stoat token type")
                        TextInput(token, { token = it }, label = "Stoat token", mono = true, masked = true, onSubmit = ::add)
                    } else if (kind == LastFmType) {
                        Label("Chats and agents can read what you listen to. To love tracks and scrobble, sign in to Last.fm after connecting. Create an API account at last.fm/api/account/create.", style = Tokens.Type.Meta, color = colors.textMuted)
                        TextInput(name, { name = it }, label = "Name", placeholder = LastFmType)
                        TextInput(account, { account = it }, label = "Last.fm username", mono = true)
                        TextInput(apiKey, { apiKey = it }, label = "Last.fm API key", mono = true)
                        TextInput(token, { token = it }, label = "Last.fm shared secret", mono = true, masked = true, onSubmit = ::add)
                    } else {
                        Label("Use a direct MCP server or a gateway such as LiteLLM. Enter its Streamable HTTP endpoint and bearer token, if required.", style = Tokens.Type.Meta, color = colors.textMuted)
                        TextInput(name, { name = it }, label = "Name", placeholder = "GitHub")
                        TextInput(url, { url = it }, label = "Streamable HTTP address", placeholder = "https://example.com/mcp", mono = true)
                        TextInput(token, { token = it }, label = "Bearer token, if the server needs one", mono = true, masked = true, onSubmit = ::add)
                    }

                    failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

                    if (signingIn) {
                        Label("Finish signing in in your browser.", style = Tokens.Type.Meta, color = colors.textMuted)
                    }

                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        MyButton(::add, text = "Connect integration", variant = ButtonVariant.Outline)

                        if (kind == McpType || kind == OutlineType) {
                            MyButton(::signIn, text = if (kind == OutlineType) "Sign in with Outline" else "Sign in with OAuth", variant = ButtonVariant.Outline)
                        }

                        MyButton(
                            ::resetForm,
                            text = "Cancel",
                            muted = true,
                        )
                    }
                }
            }
        }
    }

    removing?.let { target ->
        ConfirmDialog("Remove ${target.name}?", "Removes ${target.name}, its ${if (target.accounts.size == 1) "account" else "${target.accounts.size} accounts"} and their credentials.", "Remove", { removing = null }) {
            removing = null
            store.act {
                for (account in target.accounts) {
                    store.api.deleteChatToolServer(account.id)
                }

                servers = servers.filter { server -> target.accounts.none { it.id == server.id } }
            }
        }
    }
}
