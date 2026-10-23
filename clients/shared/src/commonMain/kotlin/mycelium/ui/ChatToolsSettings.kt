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
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.ChatToolServer
import mycelium.api.CreateChatToolServerRequest
import mycelium.api.PatchChatToolServerRequest
import mycelium.state.Store

@Composable
fun ChatToolsSettings(store: Store) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    var servers by remember { mutableStateOf<List<ChatToolServer>>(emptyList()) }
    var adding by remember { mutableStateOf(false) }
    var name by remember { mutableStateOf("") }
    var url by remember { mutableStateOf("") }
    var token by remember { mutableStateOf("") }
    var failure by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        store.act { servers = store.api.chatToolServers() }
    }

    fun replace(updated: ChatToolServer) {
        servers = servers.map { if (it.id == updated.id) updated else it }
    }

    fun add() {
        scope.launch {
            try {
                val added = store.api.createChatToolServer(CreateChatToolServerRequest(name.trim(), url.trim(), token.trim().ifEmpty { null }))
                servers = servers.filter { it.id != added.id } + added
                adding = false
                name = ""
                url = ""
                token = ""
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

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Chat tools", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Label("Chats find tools with a search and ask you before each call. Mycelium's own tools are always available.", style = Tokens.Type.Meta, color = colors.textMuted)

            ContentTable(listOf(ColumnSpec(max = 280.dp), ColumnSpec(), ColumnSpec(flexible = true), ColumnSpec(), ColumnSpec(max = 240.dp))) {
                ContentRow(header = true) {
                    Label("Server", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Token", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Tools", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Calls", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Box(Modifier)
                }

                Hairline()

                for (server in servers) {
                    ContentRow(Modifier.padding(vertical = Tokens.Space.S1_5)) {
                        Column {
                            Label(server.name, maxLines = 1)
                            Label(server.url, style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
                        }

                        Label(server.tokenHint?.let { "Ends in $it" } ?: "No token", color = colors.textMuted, maxLines = 1)
                        Label(server.tools.joinToString(", ") { it.name }.ifEmpty { "No tools found" }, color = colors.textMuted, maxLines = 2)

                        Column {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(server.enabled, "Use ${server.name} in chats", { store.act { replace(store.api.patchChatToolServer(server.id, PatchChatToolServerRequest(enabled = !server.enabled))) } }, checkedState = "On", uncheckedState = "Off")
                                Label("Use in chats", maxLines = 1)
                            }

                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(server.alwaysAllow, "Always allow tools from ${server.name}", { store.act { replace(store.api.patchChatToolServer(server.id, PatchChatToolServerRequest(alwaysAllow = !server.alwaysAllow))) } }, checkedState = "On", uncheckedState = "Off")
                                Label("Always allow", maxLines = 1)
                            }
                        }

                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1, Alignment.End)) {
                            MyButton(
                                { store.act { replace(store.api.refreshChatToolServer(server.id)) } },
                                text = "Refresh",
                                size = ButtonSize.Sm,
                                variant = ButtonVariant.Outline,
                                description = "Refresh the tools of ${server.name}",
                            )
                            MyButton(
                                {
                                    store.act {
                                        store.api.deleteChatToolServer(server.id)
                                        servers = servers.filter { it.id != server.id }
                                    }
                                },
                                text = "Remove",
                                size = ButtonSize.Sm,
                                variant = ButtonVariant.Outline,
                                description = "Remove ${server.name}",
                            )
                        }
                    }

                    Hairline()
                }
            }
        }

        if (adding) {
            Column(Modifier.widthIn(max = Tokens.Size.FormColumn), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                TextInput(name, { name = it }, label = "Name", placeholder = "GitHub")
                TextInput(url, { url = it }, label = "Streamable HTTP address", placeholder = "https://example.com/mcp", mono = true)
                TextInput(token, { token = it }, label = "Bearer token, if the server needs one", mono = true, masked = true, onSubmit = ::add)

                failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    MyButton(::add, text = "Add server", variant = ButtonVariant.Outline)
                    MyButton(
                        {
                            adding = false
                            name = ""
                            url = ""
                            token = ""
                            failure = null
                        },
                        text = "Cancel",
                        muted = true,
                    )
                }
            }
        } else {
            Row {
                MyButton({ adding = true }, text = "Add a server", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
            }
        }
    }
}
