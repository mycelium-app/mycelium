package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.RowScope
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.ConnectForgeRequest
import mycelium.api.Forge
import mycelium.api.StartChatToolServerOAuthRequest
import mycelium.state.Store

private val ForgeKinds = mapOf("GitHub" to "github", "GitLab" to "gitlab", "Forgejo" to "forgejo")

private val ForgeTokenHints = mapOf(
    "github" to "Use a token with the repo scope, or a fine-grained token with Contents, Issues and Pull requests permissions.",
    "gitlab" to "Use a token with the api scope. read_api is enough to browse only.",
    "forgejo" to "Use an access token with read and write access to user, repository and issue.",
)
private val ForgeDefaultHosts = mapOf("github" to "https://github.com", "gitlab" to "https://gitlab.com", "forgejo" to "https://codeberg.org")

private fun forgeName(forge: Forge): String = when (forge.kind) {
    "gitlab" -> "GitLab"
    "forgejo" -> "Forgejo"
    else -> "GitHub"
}

internal fun mcpAddress(forge: Forge): String = if (forge.kind == "gitlab") "${forge.baseUrl.trimEnd('/')}/api/v4/mcp" else "https://api.githubcopilot.com/mcp/"

private fun mcpOffered(kind: String, baseUrl: String): Boolean = kind != "forgejo" && (kind == "gitlab" || baseUrl.isBlank() || baseUrl.trim().trimEnd('/') == "https://github.com")

@Composable
fun ForgesSettings(store: Store) {
    val colors = MyTheme.colors
    var forges by remember { mutableStateOf<List<Forge>>(emptyList()) }
    var loaded by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    val loadFailures by store.loadFailures.collectAsState()
    val shown = forges.filter { forgeName(it).contains(query.trim(), ignoreCase = true) || it.baseUrl.contains(query.trim(), ignoreCase = true) }
    val load = { store.load("forges") { forges = store.api.forges(); loaded = true } }
    var connecting by remember { mutableStateOf(false) }
    var kind by remember { mutableStateOf("github") }
    var baseUrl by remember { mutableStateOf("") }
    var token by remember { mutableStateOf("") }
    var addMcp by remember { mutableStateOf(true) }
    var refusedMcp by remember { mutableStateOf<Forge?>(null) }
    var disconnecting by remember { mutableStateOf<Forge?>(null) }
    val platform = LocalPlatform.current
    val runner = rememberAction()

    LaunchedEffect(Unit) {
        load()
    }

    fun connect() {
        runner {
            val offered = mcpOffered(kind, baseUrl)
            val connected = store.api.connectForge(ConnectForgeRequest(kind, baseUrl.trim().ifEmpty { null }, token.trim(), addMcp = if (offered) addMcp else null))
            forges = forges.filter { it.id != connected.id } + connected
            refusedMcp = connected.takeIf { it.mcpError != null }
            connecting = false
            baseUrl = ""
            token = ""
        }
    }

    fun signInToMcp(forge: Forge) {
        runner {
            val product = forgeName(forge)
            val started = store.api.startChatToolServerOAuth(StartChatToolServerOAuthRequest(product, mcpAddress(forge), forgeId = forge.id))
            platform?.openUrl(started.authorizeUrl)
            refusedMcp = null
        }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                Label("Forges", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                MyButton({ connecting = true }, text = "Connect a forge", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
            }


            refusedMcp?.let { forge ->
                val signIn: (@Composable RowScope.() -> Unit)? = if (forge.kind == "gitlab") {
                    { MyButton({ signInToMcp(forge) }, text = "Sign in to ${forgeName(forge)}'s MCP server", size = ButtonSize.Sm, variant = ButtonVariant.Outline) }
                } else {
                    null
                }

                ErrorBanner(forge.mcpError.orEmpty(), onDismiss = { refusedMcp = null }, actions = signIn)
            }

            ListSearch(query, { query = it }, "forges")

            ListFrame("forges", loaded, "forges" in loadFailures, shown.size, load, if (query.isBlank()) "No forges. Choose Connect a forge to add one." else "No forges match.") {
                Table(
                    listOf(
                        Col("Forge", 120.dp, weight = 1f),
                        Col("Signed in as", 80.dp),
                        Col("Organisations", 120.dp, max = 320.dp, overflow = Overflow.Wrap, priority = 1),
                        ActionColumn,
                    ),
                    shown.size,
                    { row, column ->
                        val forge = shown[row]
                        val name = forgeName(forge)
                        val host = forge.baseUrl.substringAfter("://").trimEnd('/')

                        when (column) {
                            0 -> TableCell.Slot("$name $host", Tokens.Size.IconSm + Tokens.Space.S1_5 * 2) {
                                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                    BrandLogo(if (forge.kind == "gitlab") "gitlab" else "github")
                                    Label(name, maxLines = 1)
                                    Label(host, Modifier.weight(1f, fill = false), color = colors.textFaint, maxLines = 1)
                                }
                            }

                            1 -> TableCell.Text(forge.account)
                            2 -> TableCell.Text(forge.organisations.joinToString(", "), color = colors.textMuted)
                            else -> TableCell.Slot {
                                RowMenu("$name account ${forge.account} on $host") { close ->
                                    MenuItem("Disconnect", {
                                        close()

                                        if (forge.mcpServerId != null) {
                                            disconnecting = forge
                                        } else {
                                            store.act {
                                                store.api.disconnectForge(forge.id)
                                                forges = forges.filter { it.id != forge.id }
                                            }
                                        }
                                    })
                                }
                            }
                        }
                    },
                )
            }
        }

        disconnecting?.let { forge ->
            ConfirmDialog("Disconnect ${forgeName(forge)} account ${forge.account}?", "This also removes the ${forgeName(forge)} MCP integration.", "Disconnect", { disconnecting = null }) {
                disconnecting = null
                store.act {
                    store.api.disconnectForge(forge.id)
                    forges = forges.filter { it.id != forge.id }
                }
            }
        }

        if (connecting) {
            Dialog(onDismiss = {
                connecting = false
                baseUrl = ""
                token = ""
                runner.failure = null
            }) {
                Segmented(
                    ForgeKinds.keys.toList(),
                    ForgeKinds.entries.first { it.value == kind }.key,
                    { kind = ForgeKinds.getValue(it) },
                    groupLabel = "Forge",
                )
                TextInput(baseUrl, { baseUrl = it }, label = "Base URL for a self-hosted forge", placeholder = ForgeDefaultHosts.getValue(kind), mono = true)
                TextInput(token, { token = it }, label = "Personal access token", mono = true, masked = true, onSubmit = ::connect)
                Label(ForgeTokenHints.getValue(kind), style = Tokens.Type.Meta, color = colors.textMuted)

                if (mcpOffered(kind, baseUrl)) {
                    val product = ForgeKinds.entries.first { it.value == kind }.key

                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(addMcp, "Also add $product's MCP server", { addMcp = !addMcp }, checkedState = "On", uncheckedState = "Off")
                        Label("Also add $product's MCP server", maxLines = 1)
                    }

                    if (addMcp && kind == "gitlab") {
                        Label("GitLab's MCP server needs a fine-grained token with the MCP permission. With any other token, sign in afterwards.", style = Tokens.Type.Meta, color = colors.textMuted)
                    }
                }

                runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    MyButton(::connect, text = "Connect", variant = ButtonVariant.Outline)
                    MyButton(
                        {
                            connecting = false
                            baseUrl = ""
                            token = ""
                            runner.failure = null
                        },
                        text = "Cancel",
                        muted = true,
                    )
                }
            }
        }
    }
}
