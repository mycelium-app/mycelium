package mycelium.desktop

import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
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

private const val StoatAccount = """{"id":"s1","name":"Stoat","kind":"stoat","url":"https://stoat.example.com","account":"sam on stoat.example.com","token_hint":"••••cdef","enabled":true,"tools":[{"name":"get_messages","kind":"read"}],"created_at":"2026-10-22T14:00:00Z"}"""
private const val DiscordBot = """{"id":"d1","name":"Discord","kind":"discord","url":"https://discord.com/api/v10","account":"Garden bot on Allotment","token_hint":"••••abcd","enabled":true,"tools":[{"name":"get_messages","kind":"read"}],"created_at":"2026-10-22T14:00:00Z"}"""
private const val GardenHelper = """{"id":"as1","name":"Garden helper","preferences":{},"state":"idle","session_count":0,"created_at":"2026-10-01T10:00:00Z","secrets":[]}"""

class MentionTriggersTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posted = CopyOnWriteArrayList<String>()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                request.method == HttpMethod.Post && path.endsWith("/mention-triggers") -> {
                    posted += String(request.body.toByteArray())
                    respond("""{"id":"t1","server_id":"s1","assistant_id":"as1","created_at":"2026-11-25T10:00:00Z"}""", HttpStatusCode.Created, json)
                }
                path.endsWith("/mention-triggers") -> respond("[]", HttpStatusCode.OK, json)
                path.endsWith("/assistants") -> respond("[$GardenHelper]", HttpStatusCode.OK, json)
                else -> respond("[]", HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    private fun show(server: String, store: Store) {
        val servers = MyceliumJson.decodeFromString(ListSerializer(ChatToolServer.serializer()), "[$server]")

        runBlocking { store.refreshAssistants() }
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalPlatform provides QuietPlatform) {
                    Column { IntegrationPage(store, integrationsOf(servers).single(), PageActions({}, {}, {}, {}, {}, {}, {})) {} }
                }
            }
        }
    }

    @Test
    fun aStoatAccountSendsItsMentionsAndDirectMessagesToTheChosenAssistant() {
        val store = store()
        show(StoatAccount, store)

        rule.onNodeWithText("When someone mentions you or messages you directly, the message goes to the assistant you choose.").assertIsDisplayed()
        rule.onNodeWithContentDescription("Mentions for sam on stoat.example.com: Off").performClick()
        rule.onNodeWithText("Garden helper").performClick()
        rule.waitUntil(5_000) { posted.isNotEmpty() }

        assertEquals("""{"server_id":"s1","assistant_id":"as1"}""", posted.single())
    }

    @Test
    fun aDiscordBotAccountSaysItListensForTheBot() {
        show(DiscordBot, store())

        rule.onNodeWithText("When someone mentions the bot or messages it directly, the message goes to the assistant you choose.").assertIsDisplayed()
        rule.onNodeWithContentDescription("Mentions for Garden bot on Allotment: Off").assertIsDisplayed()
    }
}
