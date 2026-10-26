package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
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

private const val StoatAddress = "https://stoat.chat/api"
private const val Tls = "TLS"
private const val StartTls = "STARTTLS"
private const val UserSession = "User session"
private const val Bot = "Bot"

private fun serverType(server: ChatToolServer): String = when {
    server.settings?.get("app")?.jsonPrimitive?.content == "outline" -> OutlineType
    server.kind == "mcp" -> McpType
    server.kind == "litellm" -> LiteLlmType
    else -> "Built-in"
}

@Composable
fun ChatToolsSettings(store: Store) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    var servers by remember { mutableStateOf<List<ChatToolServer>>(emptyList()) }
    var adding by remember { mutableStateOf(false) }
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
    var failure by remember { mutableStateOf<String?>(null) }
    var renaming by remember { mutableStateOf<ChatToolServer?>(null) }
    var renamed by remember { mutableStateOf("") }
    var renameFailure by remember { mutableStateOf<String?>(null) }

    val live by store.state.collectAsState()
    val platform = LocalPlatform.current
    var signingIn by remember { mutableStateOf(false) }

    fun replace(updated: ChatToolServer) {
        servers = servers.map { if (it.id == updated.id) updated else it }
    }

    fun resetForm() {
        adding = false
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
        signingIn = false
        failure = null
    }

    LaunchedEffect(live.integrationsRevision) {
        store.act { servers = store.api.chatToolServers() }

        if (signingIn) {
            resetForm()
        }
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

    fun add() {
        scope.launch {
            try {
                val request = when (kind) {
                    LiteLlmType -> CreateChatToolServerRequest(name.trim().ifEmpty { LiteLlmType }, url.trim(), token.trim().ifEmpty { null }, kind = "litellm")
                    OutlineType -> CreateChatToolServerRequest(name.trim().ifEmpty { OutlineType }, url.trim(), token.trim().ifEmpty { null }, kind = "outline")
                    ZulipType -> CreateChatToolServerRequest(name.trim().ifEmpty { ZulipType }, url.trim(), token.trim().ifEmpty { null }, kind = "zulip", account = account.trim().ifEmpty { null })
                    EmailType -> CreateChatToolServerRequest(name.trim().ifEmpty { EmailType }, "", token.trim().ifEmpty { null }, kind = "email", account = account.trim().ifEmpty { null }, settings = emailSettings() ?: return@launch)
                    StoatType -> CreateChatToolServerRequest(name.trim().ifEmpty { StoatType }, url.trim().ifEmpty { StoatAddress }, token.trim().ifEmpty { null }, kind = "stoat", settings = JsonObject(mapOf("token_kind" to JsonPrimitive(if (tokenType == Bot) "bot" else "user"))))
                    else -> CreateChatToolServerRequest(name.trim(), url.trim(), token.trim().ifEmpty { null })
                }
                val added = store.api.createChatToolServer(request)
                servers = servers.filter { it.id != added.id } + added
                resetForm()
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
                val started = store.api.startChatToolServerOAuth(StartChatToolServerOAuthRequest(name.trim().ifEmpty { if (kind == OutlineType) OutlineType else "MCP server" }, url.trim(), if (kind == OutlineType) "outline" else null))
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

    fun rename(target: ChatToolServer) {
        scope.launch {
            try {
                replace(store.api.patchChatToolServer(target.id, PatchChatToolServerRequest(name = renamed.trim())))
                renaming = null
                renameFailure = null
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (problem: ApiException) {
                renameFailure = problem.message
            } catch (_: Exception) {
                renameFailure = "Could not reach the control plane. Check your connection and try again."
            }
        }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Label("Integrations", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
                MyButton({ adding = true }, text = "Add integration", icon = MyIcon.Plus, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }

            Label("Connect services to agents and chats. Chats ask before calling tools unless you always allow an integration.", style = Tokens.Type.Meta, color = colors.textMuted)

            if (adding) {
                Column(Modifier.widthIn(max = Tokens.Size.FormColumn), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                    Label("Add an integration", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                    Segmented(listOf(McpType, LiteLlmType, OutlineType, ZulipType, EmailType, StoatType), kind, { kind = it; failure = null }, "Integration type")

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

                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
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

            ContentTable(listOf(ColumnSpec(max = 360.dp, flexible = true), ColumnSpec(), ColumnSpec(), ColumnSpec(max = 240.dp), ColumnSpec(max = 64.dp))) {
                ContentRow(header = true) {
                    Label("Integration", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Type", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Tools", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Access", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Box(Modifier)
                }

                Hairline()

                for (server in servers) {
                    ContentRow(Modifier.padding(vertical = Tokens.Space.S1_5)) {
                        Column {
                            Label(server.name, maxLines = 1)
                            if (server.url.isNotBlank()) {
                                Label(server.url, style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
                            }

                            server.account?.let { Label(it, style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1) }
                            Label(if (server.oauth) "Signed in with OAuth" else server.tokenHint?.let { "Ends in $it" } ?: "No token", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                        }

                        Label(serverType(server), color = colors.textMuted, maxLines = 1)
                        Label(if (server.tools.isEmpty()) "No tools found" else "${server.tools.size} tools", color = colors.textMuted, maxLines = 1)

                        Column {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(server.enabled, "Use ${server.name}", { store.act { replace(store.api.patchChatToolServer(server.id, PatchChatToolServerRequest(enabled = !server.enabled))) } }, checkedState = "On", uncheckedState = "Off")
                                Label("Enabled", maxLines = 1)
                            }

                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(server.alwaysAllow, "Always allow tools from ${server.name}", { store.act { replace(store.api.patchChatToolServer(server.id, PatchChatToolServerRequest(alwaysAllow = !server.alwaysAllow))) } }, checkedState = "On", uncheckedState = "Off")
                                Label("Always allow in chats", maxLines = 1)
                            }
                        }

                        RowMenu(server.name) { close ->
                            if (server.kind == "mcp" || server.kind == "litellm") {
                                MenuItem("Refresh tools", onClick = {
                                    close()
                                    store.act { replace(store.api.refreshChatToolServer(server.id)) }
                                })
                            }

                            MenuItem("Rename", onClick = {
                                close()
                                renamed = server.name
                                renameFailure = null
                                renaming = server
                            })
                            MenuItem("Remove integration", onClick = {
                                close()
                                store.act {
                                    store.api.deleteChatToolServer(server.id)
                                    servers = servers.filter { it.id != server.id }
                                }
                            })
                        }
                    }

                    Hairline()
                }
            }

            renaming?.let { target ->
                RenameDialog("Rename integration", "Integration name", renamed, { renamed = it }, { rename(target) }, { renaming = null }, renameFailure, { renameFailure = null })
            }
        }
    }
}
