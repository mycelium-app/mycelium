package mycelium

import mycelium.api.AgentInfo
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.ModelProvider
import mycelium.api.NodeSummary
import mycelium.state.accountsElsewhere
import mycelium.state.PickerAccount
import mycelium.state.continueAccountId
import mycelium.state.pickerAccounts
import mycelium.state.reachableLogins
import mycelium.state.newChatAccounts
import mycelium.state.currentContinueLabel
import mycelium.state.currentContinueId
import kotlin.test.Test
import kotlin.test.assertEquals

private fun login(agent: String, account: String?, state: LoginState = LoginState.SignedIn, usageLeft: Int? = null) = LoginInfo(agent, account, "token", state, usageLeft = usageLeft)

private val Node = NodeSummary(
    "n1", "willow", hostname = "willow", os = "linux", arch = "x86_64", version = "v0.11.0", online = true, draining = false,
    agents = listOf(AgentInfo("claude", available = true), AgentInfo("codex", available = true)),
    logins = listOf(login("claude", "sam@example.com"), login("claude", "work@example.org", usageLeft = 4), login("claude", "old@example.org", LoginState.SignedOut), login("codex", "codex@example.com")),
)

private val Providers = listOf(ModelProvider("p1", "openai_compatible", "Ollama", models = listOf("llama3.2"), defaultModel = "llama3.2", createdAt = "2026-10-01T10:00:00Z"))

private fun conversation(agent: String, account: String? = null) =
    Conversation("c1", "u", "n1", agent, "/work/app", "Title", ConversationStatus.Done, createdAt = "2026-10-23T10:00:00Z", updatedAt = "2026-10-23T10:00:00Z", account = account)

private val Elsewhere = Node.copy(
    id = "n2", name = "atlas", agents = listOf(AgentInfo("codex", available = true)),
    logins = listOf(login("codex", "atlas@example.com"), login("claude", "unusable@example.com")),
)

private fun rows(conversation: Conversation, nodes: List<NodeSummary>): List<PickerAccount> =
    pickerAccounts(nodes, conversation.nodeId) { node, login -> continueAccountId(conversation, node, login) }

class PickerAccountsTest {
    @Test
    fun everySignedInAccountIsAListedRowWithItsHarnessAndNode() {
        val listed = rows(conversation("claude", "sam@example.com"), listOf(Node))

        assertEquals(listOf("account:sam@example.com", "account:work@example.org", "agent:codex:codex@example.com"), listed.map { it.id })
        assertEquals(listOf("claude", "claude", "codex"), listed.map { it.agent })
        assertEquals("Claude Code · work@example.org", listed[1].label)
        assertEquals(listOf(true, true, true), listed.map { it.here })
    }

    @Test
    fun aRoutedConversationHasItsRoutingChoiceChecked() {
        assertEquals("routing:any_model", currentContinueId(conversation("claude", "sam@example.com").copy(routing = "any_model")))
    }

    @Test
    fun aChatListsEveryAccountByAgentAccountAndNode() {
        val chat = conversation("chat").copy(providerId = "p1", model = "llama3.2", nodeId = "")
        val listed = rows(chat, listOf(Node))

        assertEquals(listOf("on:n1:claude:sam@example.com", "on:n1:claude:work@example.org", "on:n1:codex:codex@example.com"), listed.map { it.id })
        assertEquals(listOf(false, false, false), listed.map { it.here })
        assertEquals("n1", listed.first { it.account == "work@example.org" }.node?.id)
    }

    @Test
    fun theCurrentChoiceIsNamedForPeopleEvenWhenTheMenuDoesNotListIt() {
        assertEquals("Claude Code · Default account", currentContinueLabel(conversation("claude"), Providers))
        assertEquals("Claude Code · sam@example.com", currentContinueLabel(conversation("claude", "sam@example.com"), Providers))
        assertEquals("Ollama · qwen3", currentContinueLabel(conversation("chat").copy(providerId = "p1", model = "qwen3"), Providers))
    }

    @Test
    fun theCurrentAccountIsTheCheckedOne() {
        assertEquals("account:sam@example.com", currentContinueId(conversation("claude", "sam@example.com")))
        assertEquals("account:(default)", currentContinueId(conversation("codex")))
    }

    @Test
    fun anAccountSaysHowMuchUsageItHasLeftOrThatItIsUnknown() {
        val listed = rows(conversation("claude", "sam@example.com"), listOf(Node))

        assertEquals("4% left", listed.first { it.account == "work@example.org" }.usage)
        assertEquals("Usage unknown", listed.first { it.account == "sam@example.com" }.usage)
    }

    @Test
    fun signedOutAccountsAreLeftOut() {
        assertEquals(false, rows(conversation("claude", "sam@example.com"), listOf(Node)).any { it.account == "old@example.org" })
    }

    @Test
    fun aHarnessNotInstalledOnANodeIsLeftOut() {
        val withoutCodex = Node.copy(agents = listOf(AgentInfo("claude", available = true)))

        assertEquals(listOf("account:sam@example.com", "account:work@example.org"), rows(conversation("claude", "sam@example.com"), listOf(withoutCodex)).map { it.id })
    }

    @Test
    fun accountsOnOtherOnlineNodesAreListedWithTheirNode() {
        val listed = rows(conversation("claude", "sam@example.com"), listOf(Node, Elsewhere))
        val remote = listed.single { it.id == "on:n2:codex:atlas@example.com" }

        assertEquals("atlas", remote.node?.name)
        assertEquals(false, remote.here)
        assertEquals(false, listed.any { it.id.contains("unusable") })
    }

    @Test
    fun anOfflineNodeIsNotReachable() {
        assertEquals(false, rows(conversation("claude", "sam@example.com"), listOf(Node, Elsewhere.copy(online = false))).any { it.id.startsWith("on:") })
    }

    @Test
    fun aNewChatNamesEveryNodeAnAccountIsOnAndFlagsOneWithNoNodeOnline() {
        val offlineOnly = Node.copy(id = "n3", name = "boat", online = false, logins = listOf(login("claude", "boat@example.com")))
        val listed = newChatAccounts(listOf(Node, Elsewhere, offlineOnly))

        assertEquals("atlas", listed.single { it.account == "atlas@example.com" }.node?.name)
        assertEquals(null, listed.single { it.account == "boat@example.com" }.node)
    }

    @Test
    fun anAccountSignedOutHereButSignedInElsewhereIsOfferedForSignIn() {
        val signedOutHere = Node.copy(logins = listOf(login("claude", "atlas@example.com", LoginState.SignedOut)))
        val signedInThere = Elsewhere.copy(logins = listOf(login("claude", "atlas@example.com")))

        assertEquals(listOf("atlas@example.com"), accountsElsewhere(listOf(signedOutHere, signedInThere), signedOutHere, "claude"))
    }
}
