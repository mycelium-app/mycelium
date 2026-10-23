package mycelium.state

import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.NodeSummary

const val UnnamedAccount = "Account name not reported"

data class AccountOnNode(val nodeName: String, val state: LoginState)

data class AgentAccount(val agent: String, val label: String, val method: String, val nodes: List<AccountOnNode>)

fun agentAccounts(nodes: Collection<NodeSummary>): List<AgentAccount> {
    val grouped = linkedMapOf<Triple<String, String?, String?>, MutableList<Pair<NodeSummary, LoginInfo>>>()

    for (node in nodes.sortedBy { it.name }) {
        for (login in node.logins) {
            val account = login.account?.ifBlank { null }
            val key = Triple(login.agent, account, if (account == null) node.id else null)
            grouped.getOrPut(key) { mutableListOf() }.add(node to login)
        }
    }

    return grouped.map { (key, entries) ->
        AgentAccount(key.first, key.second ?: UnnamedAccount, entries.first().second.method, entries.map { (node, login) -> AccountOnNode(node.name, login.state) })
    }.sortedWith(compareBy({ AgentOrder.indexOf(it.agent).let { index -> if (index < 0) AgentOrder.size else index } }, { it.label }))
}

fun accountsOn(node: NodeSummary, agent: String): List<LoginInfo> = node.logins.filter { it.agent == agent }

fun switchableAccounts(node: NodeSummary?, agent: String): List<LoginInfo> = node?.let { accountsOn(it, agent) }.orEmpty().takeIf { it.size >= 2 }.orEmpty()

fun accountChoices(logins: List<LoginInfo>): List<Choice> = logins.map { login ->
    Choice(login.account.orEmpty(), login.account ?: "Default account", when (login.state) {
        LoginState.SignedIn -> null
        LoginState.Expired -> "Expired"
        LoginState.SignedOut -> "Signed out"
        LoginState.Unknown -> "Unknown"
    })
}

private val AgentOrder = listOf("claude", "codex")
