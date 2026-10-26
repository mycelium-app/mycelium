package mycelium

import mycelium.api.AgentInfo
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.ModelProvider
import mycelium.api.NodeSummary
import mycelium.state.accountChoices
import mycelium.state.accountsElsewhere
import mycelium.state.continueChoices
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

class ContinueChoicesTest {
    @Test
    fun everySignedInAccountIsListedByHarnessThenTheChatModels() {
        val choices = continueChoices(conversation("claude", "sam@example.com"), listOf(Node), Providers)

        assertEquals(listOf("account:sam@example.com", "account:work@example.org", "agent:codex:codex@example.com", "chat:p1:llama3.2"), choices.map { it.id })
        assertEquals(listOf("Claude Code", "Claude Code", "Codex", "Chat"), choices.map { it.group })
        assertEquals("Claude Code · work@example.org", choices[1].name)
    }

    @Test
    fun theCurrentAccountIsTheCheckedOne() {
        assertEquals("account:sam@example.com", currentContinueId(conversation("claude", "sam@example.com")))
        assertEquals("account:(default)", currentContinueId(conversation("codex")))
    }

    @Test
    fun anAccountNearlyOutOfUsageSaysSo() {
        val choices = continueChoices(conversation("claude", "sam@example.com"), listOf(Node), Providers)

        assertEquals("4% left", choices.first { it.id == "account:work@example.org" }.description)
    }

    @Test
    fun signedOutAccountsAreLeftOut() {
        val choices = continueChoices(conversation("claude", "sam@example.com"), listOf(Node), Providers)

        assertEquals(false, choices.any { it.id.contains("old@example.org") })
    }

    @Test
    fun aHarnessNotInstalledOnANodeIsLeftOut() {
        val withoutCodex = Node.copy(agents = listOf(AgentInfo("claude", available = true)))

        val choices = continueChoices(conversation("claude", "sam@example.com"), listOf(withoutCodex), emptyList())

        assertEquals(listOf("account:sam@example.com", "account:work@example.org"), choices.map { it.id })
    }

    @Test
    fun accountsOnOtherOnlineNodesAreListedAndSayWhichNode() {
        val choices = continueChoices(conversation("claude", "sam@example.com"), listOf(Node, Elsewhere), emptyList())
        val remote = choices.single { it.id == "on:n2:codex:atlas@example.com" }

        assertEquals("on atlas", remote.description)
        assertEquals("Codex", remote.group)
        assertEquals(false, choices.any { it.id.contains("unusable") })
    }

    @Test
    fun anOfflineNodeIsNotReachable() {
        val choices = continueChoices(conversation("claude", "sam@example.com"), listOf(Node, Elsewhere.copy(online = false)), emptyList())

        assertEquals(false, choices.any { it.id.startsWith("on:") })
    }

    @Test
    fun aChatListsEveryAccountRegardlessOfWhichProviderItUses() {
        val choices = continueChoices(conversation("chat"), listOf(Node), Providers)

        assertEquals(listOf("agent:claude:sam@example.com", "agent:claude:work@example.org", "agent:codex:codex@example.com", "chat:p1:llama3.2"), choices.map { it.id })
    }

    @Test
    fun thePickerNeverOffersSignedOutAccounts() {
        assertEquals(listOf("sam@example.com", "work@example.org"), accountChoices(Node.logins.filter { it.agent == "claude" }, emptyList(), "willow").map { it.id })
    }

    @Test
    fun anAccountSignedOutHereButSignedInElsewhereIsOfferedForSignIn() {
        val signedOutHere = Node.copy(logins = listOf(login("claude", "atlas@example.com", LoginState.SignedOut)))
        val signedInThere = Elsewhere.copy(logins = listOf(login("claude", "atlas@example.com")))

        assertEquals(listOf("atlas@example.com"), accountsElsewhere(listOf(signedOutHere, signedInThere), signedOutHere, "claude"))
    }
}
