package mycelium.desktop

import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.builtins.ListSerializer
import mycelium.api.ChatToolServer
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.Store
import mycelium.ui.IntegrationPage
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.PageActions
import mycelium.ui.integrationsOf
import org.junit.Rule
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val StoatAccount = """{"id":"s1","name":"Stoat","kind":"stoat","url":"https://stoat.example.com","account":"sam on stoat.example.com","token_hint":"••••cdef","enabled":true,"tools":[{"name":"get_messages","kind":"read"}],"created_at":"2026-10-22T14:00:00Z"}"""
private const val DiscordBot = """{"id":"d1","name":"Discord","kind":"discord","url":"https://discord.com/api/v10","account":"Garden bot","token_hint":"••••abcd","enabled":true,"tools":[{"name":"get_messages","kind":"read"}],"created_at":"2026-10-22T14:00:00Z"}"""
private const val GardenHelper = """{"id":"as1","name":"Garden helper","preferences":{},"state":"idle","session_count":0,"created_at":"2026-10-01T10:00:00Z","secrets":[]}"""
private const val Captain = """{"id":"as2","name":"Release captain","preferences":{},"state":"idle","session_count":0,"created_at":"2026-10-01T10:00:00Z","secrets":[]}"""
private const val Harbour = """{"id":"p1","node_id":"n1","name":"harbour","path":"/work/harbour","icon":"folder","created_at":"2026-10-22T10:00:00Z"}"""
private const val SamOnly = """{"id":"t1","integration":"discord","position":0,"guild_id":"70","guild_name":"Kestrel Labs","assistant_id":"as2","senders":["2"],"sender_names":["Sam"],"created_at":"2026-11-25T10:00:00Z"}"""
private const val Everyone = """{"id":"t2","integration":"discord","position":1,"assistant_id":"as1","created_at":"2026-11-25T10:01:00Z"}"""

class MentionTriggersTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val sent = CopyOnWriteArrayList<Pair<String, String>>()

    private fun store(rules: String = "[]"): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            sent += "${request.method.value} $path" to String(request.body.toByteArray())

            when {
                request.method == HttpMethod.Post && path.endsWith("/mention-triggers") -> respond(Everyone, HttpStatusCode.Created, json)
                request.method == HttpMethod.Put && path.endsWith("/mention-triggers/order") -> respond("""[${Everyone.replace("\"position\":1", "\"position\":0")},${SamOnly.replace("\"position\":0", "\"position\":1")}]""", HttpStatusCode.OK, json)
                request.method == HttpMethod.Put -> respond(SamOnly, HttpStatusCode.OK, json)
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent, json)
                path.endsWith("/mention-triggers/places") -> respond("""[{"id":"70","name":"Kestrel Labs","server_id":"d1"},{"id":"71","name":"Allotment","server_id":"d1"}]""", HttpStatusCode.OK, json)
                path.endsWith("/mention-triggers/people") -> respond("""[{"id":"3","name":"Alex"},{"id":"2","name":"Sam"}]""", HttpStatusCode.OK, json)
                path.endsWith("/mention-triggers") -> respond(rules, HttpStatusCode.OK, json)
                path.endsWith("/assistants") -> respond("[$GardenHelper,$Captain]", HttpStatusCode.OK, json)
                path.endsWith("/projects") -> respond("[$Harbour]", HttpStatusCode.OK, json)
                else -> respond("[]", HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    private fun show(server: String, store: Store) {
        val servers = MyceliumJson.decodeFromString(ListSerializer(ChatToolServer.serializer()), "[$server]")

        runBlocking {
            store.refreshAssistants()
            store.refreshProjects()
            store.refreshMentionTriggers()
        }
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalPlatform provides QuietPlatform) {
                    Column { IntegrationPage(store, integrationsOf(servers).single(), PageActions({}, {}, {}, {}, {}, {}, {})) {} }
                }
            }
        }
    }

    private fun body(request: String): String {
        rule.waitUntil(5_000) { sent.any { it.first == request } }

        return sent.last { it.first == request }.second
    }

    @Test
    fun rulesAreListedInOrderAndAnythingElseGoesNowhere() {
        show(DiscordBot, store("[$SamOnly,$Everyone]"))

        rule.onNodeWithText("When someone mentions the bot or messages it directly, the first rule that matches decides where it goes.").assertIsDisplayed()
        rule.onNodeWithContentDescription("Rule 1: Kestrel Labs, Sam, goes to Release captain").assertIsDisplayed()
        rule.onNodeWithContentDescription("Rule 2: Any server, Everyone, goes to Garden helper").assertIsDisplayed()
        rule.onNodeWithText("Anything else").assertIsDisplayed()
        rule.onNodeWithText("Nobody").assertIsDisplayed()
    }

    @Test
    fun anAddedRuleOpensAtTheEndAndIsSavedWithItsAssistantWhenDone() {
        show(StoatAccount, store())

        rule.onNodeWithText("Add rule").performClick()
        rule.onNodeWithContentDescription("Goes to: Choose").performClick()
        rule.onNodeWithText("Garden helper").performClick()
        rule.onNodeWithText("Done").performClick()

        assertEquals("""{"integration":"stoat","assistant_id":"as1"}""", body("POST /api/v1/mention-triggers"))
    }

    @Test
    fun aRuleWithNowhereToGoSaysSoInsteadOfSaving() {
        show(DiscordBot, store())

        rule.onNodeWithText("Add rule").performClick()
        rule.onNodeWithText("Done").performClick()

        rule.onNodeWithText("Choose where the messages go: an assistant, or an agent in a project").assertIsDisplayed()
        assertTrue(sent.none { it.first == "POST /api/v1/mention-triggers" })
    }

    @Test
    fun aRuleCanSendMentionsToAnAgentInAProject() {
        show(DiscordBot, store())

        rule.onNodeWithText("Add rule").performClick()
        rule.onNodeWithContentDescription("Goes to: Choose").performClick()
        rule.onNodeWithText("Claude Code in harbour").performClick()
        rule.onNodeWithText("Done").performClick()

        assertEquals("""{"integration":"discord","project_id":"p1","agent":"claude"}""", body("POST /api/v1/mention-triggers"))
    }

    @Test
    fun anOpenRuleTakesAServerAndPeopleFromIt() {
        show(DiscordBot, store("[$Everyone]"))

        rule.onNodeWithContentDescription("Rule 1: Any server, Everyone, goes to Garden helper").performClick()
        rule.onNodeWithContentDescription("In: Any server").performClick()
        rule.waitUntil(5_000) { rule.onAllNodes(hasText("Allotment")).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Allotment").performClick()
        rule.onNodeWithContentDescription("Add a person").performTextInput("a")
        rule.waitUntil(5_000) { rule.onAllNodes(hasText("Alex")).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Alex").performClick()
        rule.onNodeWithContentDescription("Remove Alex").assertIsDisplayed()
        rule.onNodeWithText("Done").performClick()

        assertEquals("""{"integration":"discord","guild_id":"71","guild_name":"Allotment","assistant_id":"as1","senders":["3"],"sender_names":["Alex"]}""", body("PUT /api/v1/mention-triggers/t2"))
    }

    @Test
    fun aRuleMovesUpFromItsMenuWithoutDragging() {
        show(DiscordBot, store("[$SamOnly,$Everyone]"))

        rule.onNodeWithContentDescription("More for rule 2").performClick()
        rule.onNodeWithText("Move up").performClick()

        assertEquals("""{"integration":"discord","ids":["t2","t1"]}""", body("PUT /api/v1/mention-triggers/order"))
        rule.waitUntil(5_000) { rule.onAllNodes(androidx.compose.ui.test.hasContentDescription("Rule 1: Any server, Everyone, goes to Garden helper")).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aRuleIsRemovedFromInsideTheOpenRow() {
        show(DiscordBot, store("[$SamOnly]"))

        rule.onNodeWithContentDescription("Rule 1: Kestrel Labs, Sam, goes to Release captain").performClick()
        rule.onNodeWithText("Remove rule").performClick()

        body("DELETE /api/v1/mention-triggers/t1")
    }
}
