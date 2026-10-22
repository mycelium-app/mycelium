package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.api.Conversation
import mycelium.api.CreateConversationRequest
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.isLaptop

fun startableNodes(state: AppState) = state.nodes.values.filter { it.online && !it.draining }.sortedBy { it.name }

@Composable
fun NewConversationScreen(store: Store, state: AppState, onCreated: (Conversation) -> Unit) {
    val colors = MyTheme.colors
    val nodes = startableNodes(state)
    var nodeId by remember { mutableStateOf<String?>(null) }
    var agent by remember { mutableStateOf<String?>(null) }
    var cwd by remember { mutableStateOf("") }
    var prompt by remember { mutableStateOf("") }
    var cwdError by remember { mutableStateOf<String?>(null) }
    var nodeMenu by remember { mutableStateOf(false) }

    val node = nodes.firstOrNull { it.id == nodeId } ?: nodes.firstOrNull()
    val agents = node?.agents?.filter { it.available }.orEmpty()
    val chosenAgent = agents.firstOrNull { it.agent == agent }?.agent ?: agents.firstOrNull()?.agent

    fun create() {
        if (node == null || chosenAgent == null) {
            return
        }

        if (cwd.isBlank()) {
            cwdError = "Enter the folder the agent should work in, for example /home/you/project."
            return
        }

        cwdError = null
        store.act {
            val created = store.api.createConversation(
                CreateConversationRequest(node.id, chosenAgent, cwd.trim(), prompt = prompt.trim().ifEmpty { null }),
            )
            onCreated(created)
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("New conversation")
        }

        if (node == null) {
            Label("No node is online. Join a node from Nodes, or wait for one to reconnect.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)

            return@Column
        }

        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.TopCenter) {
            Column(
                Modifier.widthIn(max = Tokens.Size.Page).fillMaxWidth().padding(Tokens.Space.S4),
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4),
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label("Node", style = Tokens.Type.MetaStrong, color = colors.textMuted)

                    Box {
                        MyButton(
                            { nodeMenu = true },
                            text = node.name,
                            icon = if (isLaptop(node.os)) MyIcon.Laptop else MyIcon.Server,
                            description = "Node: ${node.name}",
                            variant = ButtonVariant.Outline,
                            trailingIcon = MyIcon.ChevronDown,
                        )

                        if (nodeMenu) {
                            MenuPopup(onDismiss = { nodeMenu = false }) {
                                MenuLabel("Run on")

                                for (option in nodes) {
                                    MenuItem(
                                        option.name,
                                        onClick = { nodeId = option.id; nodeMenu = false },
                                        checked = option.id == node.id,
                                        leading = if (isLaptop(option.os)) MyIcon.Laptop else MyIcon.Server,
                                    )
                                }
                            }
                        }
                    }
                }

                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label("Agent", style = Tokens.Type.MetaStrong, color = colors.textMuted)

                    if (agents.isEmpty()) {
                        Label("No agent is installed on ${node.name}. Install Claude Code or Codex there, then try again.", color = colors.statusFailed)
                    } else {
                        Segmented(
                            options = agents.map { agentName(it.agent) },
                            selected = agentName(chosenAgent.orEmpty()),
                            onSelect = { picked -> agent = agents.first { agentName(it.agent) == picked }.agent },
                            groupLabel = "Agent",
                        )
                    }
                }

                TextInput(cwd, { cwd = it }, label = "Folder", placeholder = "/home/you/project", mono = true, error = cwdError, onSubmit = ::create)

                TextInput(prompt, { prompt = it }, label = "First message (optional)", placeholder = "Ask, plan or build", onSubmit = ::create)

                Row {
                    MyButton(::create, text = "Start conversation", variant = ButtonVariant.Primary, size = ButtonSize.Lg)
                }
            }
        }
    }
}
