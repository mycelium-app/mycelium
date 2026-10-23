package mycelium.state

import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.NodeSummary
import kotlin.random.Random

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

const val DefaultAccountId = "(default)"

const val SignInPrefix = "sign in:"

fun accountChoiceId(login: LoginInfo): String = login.account ?: DefaultAccountId

fun accountsElsewhere(nodes: Collection<NodeSummary>, node: NodeSummary, agent: String): List<String> {
    val here = accountsOn(node, agent).mapNotNull { it.account }.toSet()

    return nodes.filter { it.id != node.id }.flatMap { accountsOn(it, agent) }.mapNotNull { it.account }.distinct().filter { it !in here }.sorted()
}

fun accountChoices(logins: List<LoginInfo>, elsewhere: List<String>, nodeName: String): List<Choice> =
    logins.map { login ->
        val left = login.usageLeft

        Choice(accountChoiceId(login), login.account ?: "Default account", when (login.state) {
            LoginState.SignedIn -> if (left == null) "Usage unknown" else "$left% left"
            LoginState.Expired -> "Expired"
            LoginState.SignedOut -> "Signed out"
            LoginState.Unknown -> "Unknown"
        }, usedPercent = left?.let { 100 - it }.takeIf { login.state == LoginState.SignedIn })
    } + elsewhere.map { Choice(SignInPrefix + it, it, "Sign in on $nodeName") }

fun accountMenuTitle(agent: String): String = when (agent) {
    "claude" -> "Claude account"
    "codex" -> "Codex account"
    else -> "Account"
}

fun newAccountName(): String = "account-${Random.nextInt(1000, 10000)}"

private val AgentOrder = listOf("claude", "codex")
