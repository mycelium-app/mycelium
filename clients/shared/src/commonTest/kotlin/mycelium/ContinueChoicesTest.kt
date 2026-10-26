package mycelium

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import mycelium.api.AgentInfo
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.ModelProvider
import mycelium.api.NodeSummary
import mycelium.state.continueChoices
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

private fun continued(from: JsonObject) = Event("c1", 5, EventKinds.Continued, buildJsonObject { put("kind", "handoff"); put("agent", "chat"); put("from", from) })

class ContinueChoicesTest {
    @Test
    fun aHarnessConversationListsOtherAccountsThenTheOtherHarnessThenChatModels() {
        val choices = continueChoices(conversation("claude", "sam@example.com"), Node, Providers, emptyList())

        assertEquals(listOf("account:work@example.org", "agent:codex:codex@example.com", "chat:p1:llama3.2"), choices.map { it.id })
        assertEquals(listOf("Claude Code · work@example.org", "Codex · codex@example.com", "Ollama · llama3.2"), choices.map { it.name })
    }

    @Test
    fun anAccountNearlyOutOfUsageSaysSo() {
        val choices = continueChoices(conversation("claude", "sam@example.com"), Node, Providers, emptyList())

        assertEquals("4% left", choices.first().description)
    }

    @Test
    fun signedOutAccountsAreLeftOut() {
        val choices = continueChoices(conversation("claude", "sam@example.com"), Node, Providers, emptyList())

        assertEquals(false, choices.any { it.id.contains("old@example.org") })
    }

    @Test
    fun theOtherHarnessIsLeftOutWhenItIsNotInstalledOnTheNode() {
        val withoutCodex = Node.copy(agents = listOf(AgentInfo("claude", available = true)))

        val choices = continueChoices(conversation("claude", "sam@example.com"), withoutCodex, emptyList(), emptyList())

        assertEquals(listOf("account:work@example.org"), choices.map { it.id })
    }

    @Test
    fun aChatListsTheHarnessItCameFrom() {
        val from = buildJsonObject { put("agent", "claude"); put("account", "sam@example.com") }

        val choices = continueChoices(conversation("chat"), null, Providers, listOf(continued(from)))

        assertEquals(listOf("agent:claude:sam@example.com"), choices.map { it.id })
        assertEquals("Claude Code · sam@example.com", choices.single().name)
    }

    @Test
    fun aChatThatNeverRanOnAHarnessHasNothingToContinueOn() {
        assertEquals(emptyList(), continueChoices(conversation("chat"), null, Providers, emptyList()))
    }
}
