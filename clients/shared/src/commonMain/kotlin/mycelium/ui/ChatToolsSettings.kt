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
private const val CalDavType = "Calendar"
private const val DiscordType = "Discord"
private const val FreeAgentType = "FreeAgent"
private const val ActualType = "Actual Budget"
private const val PaperlessType = "Paperless-ngx"
private const val CoolifyType = "Coolify"
private const val TandoorType = "Tandoor"

private sealed interface IntegrationsView {
    data object Home : IntegrationsView

    data object Catalogue : IntegrationsView

    data class Page(val key: String) : IntegrationsView

    data class Account(val key: String, val id: String) : IntegrationsView

    data class Activity(val key: String?) : IntegrationsView
}

private data class SetupRequest(val kind: String, val address: String?)

private fun flowId(kind: String): String = when (kind) {
    OutlineType -> "outline"
    LiteLlmType -> "litellm"
    LastFmType -> "lastfm"
    ZulipType -> "zulip"
    EmailType -> "email"
    StoatType -> "stoat"
    CalDavType -> "caldav"
    DiscordType -> "discord"
    FreeAgentType -> "freeagent"
    ActualType -> "actual"
    PaperlessType -> "paperless"
    CoolifyType -> "coolify"
    TandoorType -> "tandoor"
    else -> "mcp"
}

private fun formKind(entry: CatalogueEntry): String = when (entry.kind) {
    "email" -> EmailType
    "zulip" -> ZulipType
    "stoat" -> StoatType
    "outline" -> OutlineType
    "lastfm" -> LastFmType
    "litellm" -> LiteLlmType
    "caldav" -> CalDavType
    "discord" -> DiscordType
    "freeagent" -> FreeAgentType
    "actual" -> ActualType
    "paperless" -> PaperlessType
    "coolify" -> CoolifyType
    "tandoor" -> TandoorType
    else -> McpType
}

@Composable
fun ChatToolsSettings(store: Store) {
    var servers by remember { mutableStateOf<List<ChatToolServer>>(emptyList()) }
    var forges by remember { mutableStateOf<List<Forge>>(emptyList()) }
    var view by remember { mutableStateOf<IntegrationsView>(IntegrationsView.Home) }
    var removing by remember { mutableStateOf<Integration?>(null) }
    var flow by remember { mutableStateOf<SetupRequest?>(null) }

    val live by store.state.collectAsState()
    val platform = LocalPlatform.current
    val openConversation = LocalOpenConversation.current
    var signingIn by remember { mutableStateOf(false) }
    var loaded by remember { mutableStateOf(false) }

    LaunchedEffect(servers) {
        if (loaded) {
            store.shareChatToolServers(servers)
        }
    }

    LaunchedEffect(live.integrationsRevision) {
        val known = servers.map { it.id }.toSet()

        store.act {
            servers = store.api.chatToolServers()
            loaded = true

            if (signingIn) {
                signingIn = false
                integrationsOf(servers).firstOrNull { integration -> integration.accounts.any { it.id !in known } }?.let { view = IntegrationsView.Page(it.key) }
            }
        }

        store.act { forges = store.api.forges() }
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
        } else if (target.kind == "freeagent") {
            store.act {
                val start = store.api.startFreeAgentSignIn(target.id)
                platform?.openUrl(start.authorizeUrl)
            }
        } else if (target.oauth) {
            store.act {
                val started = store.api.startChatToolServerOAuth(StartChatToolServerOAuthRequest(target.name, target.url, if (target.settings?.get("app")?.jsonPrimitive?.content == "outline") "outline" else null, forgeId = target.forgeId))
                platform?.openUrl(started.authorizeUrl)
            }
        }
    }

    fun addForgeServer(forge: Forge) {
        store.act {
            val refused = store.api.linkForgeMcp(forge.id).mcpError ?: return@act

            if (forge.kind != "gitlab") {
                throw ApiException(422, "mcp_refused", refused)
            }

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

                        if (only != null && (only.oauth || only.kind == "lastfm" || only.kind == "freeagent")) {
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
                { entry -> flow = SetupRequest(formKind(entry), null) },
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
                                val kind = formKind(entryFor(integration) ?: AnyMcpEntry)
                                flow = SetupRequest(kind, integration.accounts.first().url.takeIf { kind !in setOf(LastFmType, EmailType, DiscordType, FreeAgentType) })
                            },
                            ::replace,
                            { removed -> servers = servers.filter { it.id != removed.id } },
                            ::reauthenticate,
                            { view = IntegrationsView.Activity(it.key) },
                            { view = IntegrationsView.Account(integration.key, it.id) },
                        ),
                        { openConversation?.invoke(it) },
                    )
                }
            }

            is IntegrationsView.Account -> {
                val integration = integrations.find { it.key == current.key }
                val account = integration?.accounts?.find { it.id == current.id }

                if (integration == null) {
                    view = IntegrationsView.Home
                } else if (account == null) {
                    view = IntegrationsView.Page(integration.key)
                } else {
                    AccountPage(
                        store,
                        integration,
                        account,
                        PageActions(
                            { view = IntegrationsView.Home },
                            {},
                            ::replace,
                            { removed -> servers = servers.filter { it.id != removed.id } },
                            ::reauthenticate,
                            { view = IntegrationsView.Activity(it.key) },
                            {},
                        ),
                        { view = IntegrationsView.Page(integration.key) },
                        { openConversation?.invoke(it) },
                    )
                }
            }

            is IntegrationsView.Activity -> IntegrationsActivity(store, integrations, integrations.find { it.key == current.key }, { view = IntegrationsView.Home }, { openConversation?.invoke(it) })
        }
    }

    flow?.let { request ->
        SetupFlow(
            store,
            flowId(request.kind),
            request.address,
            servers.map { integrationSlug(it.name) }.toSet(),
            { added ->
                servers = servers.filter { it.id != added.id } + added

                if (request.kind == LastFmType || request.kind == FreeAgentType) {
                    view = IntegrationsView.Page(integrationsOf(servers).first { integration -> integration.accounts.any { it.id == added.id } }.key)
                }
            },
            { added ->
                flow = null
                view = IntegrationsView.Page(integrationsOf(servers).first { integration -> integration.accounts.any { it.id == added.id } }.key)
            },
            {
                signingIn = true
                flow = null
            },
            { flow = null },
        )
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
