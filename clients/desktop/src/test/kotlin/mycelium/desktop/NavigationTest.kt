package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test

private object NavigationPlatform : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

class NavigationTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun tasksAndSchedulesSitUnderNodesInTheSidebarAndOpenTheirPages() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { respond("[]", HttpStatusCode.OK, json) }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        rule.setContent { MyceliumTheme(true) { Shell(store, NavigationPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Nodes) } }

        rule.onNodeWithText("Tasks").performClick()
        rule.onNodeWithText("Add a task").assertExists()
        rule.onNodeWithText("Schedules").performClick()
        rule.onNodeWithText("New schedule").assertExists()
    }
}
