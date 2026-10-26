package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ExperimentsSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test

class ExperimentsSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    @Test
    fun eachExperimentSaysWhatItTurnsOn() {
        val engine = MockEngine { respond("{}", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json")) }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        rule.setContent { MyceliumTheme(true) { ExperimentsSettings(store) } }

        rule.onNodeWithText("Voice input").assertIsDisplayed()
        rule.onNodeWithText("Dictate into the composer. On Android and the web, audio may go to Google.").assertIsDisplayed()
        rule.onNodeWithText("Keep memories apart by model class and group.").assertIsDisplayed()
    }
}
