package mycelium

import mycelium.api.AgentInfo
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.NodeSummary
import mycelium.state.agentPicks
import mycelium.state.chatSections
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.time.Instant

private fun login(agent: String, account: String, state: LoginState = LoginState.SignedIn) = LoginInfo(agent, account, "token", state)

private fun node(id: String, name: String, online: Boolean, vararg logins: LoginInfo) =
    NodeSummary(id, name, hostname = name, os = "linux", arch = "x86_64", version = "v0.15.0", online = online, draining = false, agents = listOf(AgentInfo("claude", available = true)), logins = logins.toList())

class ChatAgentsTest {
    @Test
    fun anyAccountComesFirstThenEachAccountNamingTheNodesItIsOnlineOn() {
        val picks = agentPicks(listOf(node("1", "laptop", true, login("claude", "sam@example.com")), node("2", "desktop", true, login("claude", "sam@example.com")), node("3", "server", false, login("claude", "work@example.org"))))

        assertEquals(listOf("Claude Code, any account", "Claude Code · sam@example.com", "Claude Code · work@example.org"), picks.map { it.label })
        assertEquals(listOf("most usage left", "online on desktop, laptop", "no node online"), picks.map { it.hint })
    }

    @Test
    fun signedOutAccountsAreNotOffered() {
        assertEquals(emptyList(), agentPicks(listOf(node("1", "laptop", true, login("claude", "sam@example.com", LoginState.SignedOut)))))
    }

    @Test
    fun aChatOnAnAgentIsListedWithTheChats() {
        val scratch = Conversation("c1", "u", "n1", "claude", ".mycelium-assistants/c1", "Title", ConversationStatus.Done, createdAt = "2026-10-23T10:00:00Z", updatedAt = "2026-10-23T10:00:00Z", scratch = true)

        assertEquals(listOf("c1"), chatSections(listOf(scratch), Instant.parse("2026-10-23T12:00:00Z")).flatMap { it.rows }.map { it.id })
    }
}
