package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
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
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.ConnectForgeRequest
import mycelium.api.Forge
import mycelium.state.Store

private val ForgeKinds = mapOf("GitHub" to "github", "GitLab" to "gitlab")

private val ForgeDefaultHosts = mapOf("github" to "https://github.com", "gitlab" to "https://gitlab.com")

@Composable
fun ForgesSettings(store: Store) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    var forges by remember { mutableStateOf<List<Forge>>(emptyList()) }
    var connecting by remember { mutableStateOf(false) }
    var kind by remember { mutableStateOf("github") }
    var baseUrl by remember { mutableStateOf("") }
    var token by remember { mutableStateOf("") }
    var failure by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        store.act { forges = store.api.forges() }
    }

    fun connect() {
        scope.launch {
            try {
                val connected = store.api.connectForge(ConnectForgeRequest(kind, baseUrl.trim().ifEmpty { null }, token.trim()))
                forges = forges.filter { it.id != connected.id } + connected
                connecting = false
                baseUrl = ""
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
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Forges", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Label("Forge", Modifier.weight(2f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("Signed in as", Modifier.weight(1.5f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("Organisations", Modifier.weight(2.5f), Tokens.Type.MetaStrong, colors.textMuted)
                Box(Modifier.weight(1f))
            }

            Hairline()

            for (forge in forges) {
                val name = if (forge.kind == "gitlab") "GitLab" else "GitHub"
                val host = forge.baseUrl.substringAfter("://").trimEnd('/')

                Row(
                    Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Row(Modifier.weight(2f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                        Icon(MyIcon.Forge, colors.textMuted, Tokens.Size.IconSm)
                        Label(name, maxLines = 1)
                        Label(host, Modifier.weight(1f, fill = false), color = colors.textFaint, maxLines = 1)
                    }

                    Label(forge.account, Modifier.weight(1.5f), maxLines = 1)
                    Label(forge.organisations.joinToString(", "), Modifier.weight(2.5f), color = colors.textMuted)

                    Row(Modifier.weight(1f), horizontalArrangement = Arrangement.End) {
                        MyButton(
                            {
                                store.act {
                                    store.api.disconnectForge(forge.id)
                                    forges = forges.filter { it.id != forge.id }
                                }
                            },
                            text = "Disconnect",
                            size = ButtonSize.Sm,
                            variant = ButtonVariant.Outline,
                            description = "Disconnect $name account ${forge.account} on $host",
                        )
                    }
                }

                Hairline()
            }
        }

        if (connecting) {
            Column(Modifier.widthIn(max = Tokens.Size.FormColumn), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Segmented(
                    ForgeKinds.keys.toList(),
                    ForgeKinds.entries.first { it.value == kind }.key,
                    { kind = ForgeKinds.getValue(it) },
                    groupLabel = "Forge",
                )
                TextInput(baseUrl, { baseUrl = it }, label = "Base URL for a self-hosted forge", placeholder = ForgeDefaultHosts.getValue(kind), mono = true)
                TextInput(token, { token = it }, label = "Personal access token", mono = true, masked = true, onSubmit = ::connect)

                failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    MyButton(::connect, text = "Connect", variant = ButtonVariant.Outline)
                    MyButton(
                        {
                            connecting = false
                            baseUrl = ""
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
                MyButton({ connecting = true }, text = "Connect a forge", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
            }
        }
    }
}
