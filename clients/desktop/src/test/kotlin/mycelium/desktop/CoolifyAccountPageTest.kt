package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import kotlinx.serialization.json.Json
import mycelium.api.ChatToolServer
import mycelium.ui.AccountPage
import mycelium.ui.Integration
import mycelium.ui.IntegrationCategory
import mycelium.ui.LocalAppsOpener
import mycelium.ui.MyceliumTheme
import mycelium.ui.PageActions
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Options = """{"projects":[{"uuid":"p1","name":"Mycelium apps"},{"uuid":"p2","name":"Homelab"}],"servers":[{"uuid":"s1","name":"atlas-box","wildcard_domain":"apps.example.com"}]}"""

class CoolifyAccountPageTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = mutableListOf<Pair<String, String>>()
    private var opened = 0
    private var removed = false

    private fun open() {
        val store = appsStore(scopes.create()) { request ->
            val path = request.url.encodedPath
            requests += "${request.method.value} $path" to ""

            when {
                path.endsWith("/coolify-options") -> json(Options)
                path.endsWith("/coolify-target") -> json(CoolifyAccount.replace("Mycelium apps", "Homelab"))
                request.method.value == "DELETE" -> respond204()
                else -> json("[]")
            }
        }
        val account = Json { ignoreUnknownKeys = true }.decodeFromString(ChatToolServer.serializer(), CoolifyAccount)
        val integration = Integration("coolify", "Coolify", "coolify", IntegrationCategory.Developer, listOf(account), null)
        val actions = PageActions({}, {}, {}, { removed = true }, {}, {}, {})

        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalAppsOpener provides { opened += 1 }) {
                    AccountPage(store, integration, account, actions, {}, {})
                }
            }
        }
    }

    @Test
    fun theOverviewShowsWhereAppsGoAndOpensTheAppsPage() {
        open()

        rule.onNodeWithText("Mycelium apps").assertExists()
        rule.onNodeWithText("atlas-box").assertExists()
        rule.onNodeWithText("*.apps.example.com").assertExists()
        rule.onNodeWithText("Open Apps").performClick()

        assertEquals(1, opened)
    }

    @Test
    fun changingWhereAppsGoOffersTheProjectsFoundAndSavesTheChoice() {
        open()

        rule.onNodeWithText("Change where apps go").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Agents create every app in this project. Apps in other projects stay out of their reach.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { requests.any { it.first.endsWith("/coolify-target") } }
        assertTrue(requests.any { it.first == "PUT /api/v1/chat-tool-servers/c1/coolify-target" })
    }

    @Test
    fun removingTheAccountSaysTheAppsKeepRunning() {
        open()

        rule.onNodeWithText("Remove account").performClick()

        rule.onNodeWithText("Removes this Coolify account and its credentials. Apps keep running in Coolify, and Apps leaves the sidebar.").assertExists()
    }
}
