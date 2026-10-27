package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.serialization.builtins.ListSerializer
import mycelium.api.ChatToolServer
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.Store
import mycelium.ui.AccountPage
import mycelium.ui.DecisionsSettings
import mycelium.ui.SetupFlow
import mycelium.ui.IntegrationActions
import mycelium.ui.IntegrationPage
import mycelium.ui.IntegrationsCatalogue
import mycelium.ui.IntegrationsHome
import mycelium.ui.MyTheme
import mycelium.ui.PageActions
import mycelium.ui.Tokens
import mycelium.ui.integrationsOf
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

class IntegrationsRenderTest {
    @get:Rule
    val scopes = TestScopes()

    private val servers = MyceliumJson.decodeFromString(ListSerializer(ChatToolServer.serializer()), "[$DocsServer,$MailA,$MailB,$ZulipLocked,$TrackerOff,$StoatExpired]")

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")

        return Store(MyceliumApi("https://mycelium.test", "token", MockEngine { respond("[]", HttpStatusCode.OK, json) }), scopes.create())
    }

    private fun screen(name: String, dark: Boolean, phone: Boolean, content: @androidx.compose.runtime.Composable () -> Unit) =
        renderAt(name, dark, phone) {
            Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent).padding(24.dp)) {
                Column(Modifier.widthIn(max = Tokens.Size.Page)) { content() }
            }
        }

    @Test
    fun theHomeCatalogueAndAnIntegrationsPageRenderOnADesktopAndAPhone() {
        val store = store()
        val actions = IntegrationActions({}, {}, {}, {})
        val pageActions = PageActions({}, {}, {}, {}, {}, {}, {})

        for (phone in listOf(false, true)) {
            for (dark in listOf(true, false)) {
                assertTrue(screen("integrations-home", dark, phone) { IntegrationsHome(servers, actions, {}, {}) }.length() > 5_000)
                assertTrue(screen("integrations-catalogue", dark, phone) { IntegrationsCatalogue(integrationsOf(servers), emptyList(), {}, {}, {}, {}) }.length() > 5_000)
                assertTrue(screen("integrations-page", dark, phone) { IntegrationPage(store, integrationsOf(servers).first { it.name == "Email" }, pageActions) {} }.length() > 5_000)
            }
        }
    }

    @Test
    fun theSetupDialogAccountPageAndDecisionsSettingsRenderOnADesktopAndAPhone() {
        val store = store()
        val pageActions = PageActions({}, {}, {}, {}, {}, {}, {})
        val email = integrationsOf(servers).first { it.name == "Email" }

        for (phone in listOf(false, true)) {
            for (dark in listOf(true, false)) {
                assertTrue(screen("setup-flow", dark, phone) { SetupFlow(store, "mcp", "https://example.com/mcp", emptySet(), {}, {}, {}, {}) }.length() > 5_000)
                assertTrue(
                    screen("setup-flow-over-catalogue", dark, phone) {
                        IntegrationsCatalogue(integrationsOf(servers), emptyList(), {}, {}, {}, {})
                        SetupFlow(store, "mcp", "https://example.com/mcp", emptySet(), {}, {}, {}, {})
                    }.length() > 5_000,
                )
                assertTrue(screen("setup-flow-email", dark, phone) { SetupFlow(store, "email", null, emptySet(), {}, {}, {}, {}) }.length() > 5_000)
                assertTrue(screen("integration-account", dark, phone) { AccountPage(store, email, email.accounts.first(), pageActions, {}) {} }.length() > 5_000)
                assertTrue(screen("decisions-settings", dark, phone) { DecisionsSettings(store) }.length() > 3_000)
            }
        }
    }
}
