package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import mycelium.ui.AppPage
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class AppPageTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val platform = AppsPlatform()
    private val requests = mutableListOf<Pair<String, String>>()

    private fun show(tab: String = "Overview", deployment: String? = null) {
        val store = appsStore(scopes.create()) { request ->
            val path = request.url.encodedPath
            requests += "${request.method.value} $path" to request.url.parameters.entries().joinToString("&") { "${it.key}=${it.value.first()}" }

            when {
                path.endsWith("/apps/a1/deployments") -> json(DeploymentsJson)
                path.endsWith("/apps/a1/envs") -> json(EnvsJson)
                path.endsWith("/apps/a1/logs") -> json(LogsJson)
                path.contains("/apps/a1/envs/") && request.method.value == "PUT" -> json(EnvsJson.removePrefix("[").substringBefore("},{") + "}")
                path.endsWith("/apps/a1") -> json(AppDetailJson)
                else -> json("{}")
            }
        }

        rule.setContent { MyceliumTheme(true) { AppPage(store, store.state.value, platform, "a1", tab, deployment, onBack = {}, onOpenConversation = {}) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("harbour-status").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun tab(label: String) = rule.onNode(hasText(label) and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab))

    @Test
    fun theFourTabsAreFoundByRoleAndSwitchingShowsTheirContent() {
        show()

        listOf("Overview", "Deployments", "Logs", "Environment").forEach { tab(it).assertExists() }
        tab("Overview").assertIsSelected()
        rule.onNodeWithText("Delete app").assertExists()

        tab("Deployments").performClick()
        tab("Deployments").assertIsSelected()
        rule.onNodeWithText("c81d2f0 First cut of the status page").assertExists()

        tab("Environment").performClick()
        rule.onNodeWithText("Add variable").assertExists()

        tab("Logs").performClick()
        rule.onNodeWithText("Follow").assertExists()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("14:03:51 INFO Listening on :8080").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aDeploymentRowOpensItsBuildLog() {
        show()

        tab("Deployments").performClick()
        rule.onNodeWithText("c81d2f0 First cut of the status page").performClick()

        tab("Logs").assertIsSelected()
        rule.waitUntil(5_000) { requests.any { it.first.endsWith("/apps/a1/logs") && it.second.contains("kind=build") && it.second.contains("deployment=d0") } }
    }

    @Test
    fun searchingLogsKeepsMatchingLinesAndEscapeClearsTheSearch() {
        show("Logs")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("14:03:51 INFO Listening on :8080").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Search logs").performTextInput("error")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("14:03:51 INFO Listening on :8080").fetchSemanticsNodes().isEmpty() }
        rule.onNodeWithText("14:31:12 ERROR Could not reach the backup database").assertExists()

        rule.onNodeWithContentDescription("Search logs").performKeyInput { pressKey(Key.Escape) }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("14:03:51 INFO Listening on :8080").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theFollowButtonReportsWhetherItIsOn() {
        show("Logs")

        rule.onNode(hasText("Follow")).performClick()
        rule.onNode(hasText("Follow")).assertIsSelected()
    }

    @Test
    fun secretVariablesHaveNoShowButtonAndPlainOnesDo() {
        show("Environment")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("DATABASE_URL").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("Show the value of DATABASE_URL").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("Show the value of PORT").performClick()
        rule.onNodeWithText("8080").assertExists()
        rule.onNodeWithText("DATABASE_URL changed after the last deploy. Redeploy to use it.").assertExists()
    }

    @Test
    fun savingAnEmptyValueAsksForOneAndSendsNothing() {
        show("Environment")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("DATABASE_URL").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Change DATABASE_URL").performClick()
        rule.onNodeWithText("Save").performClick()

        rule.onNodeWithText("Enter a value for DATABASE_URL").assertExists()
        assertTrue(requests.none { it.first.startsWith("PUT") })
    }

    @Test
    fun savingAValueReplacesTheWholeVariable() {
        show("Environment")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("PORT").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Change PORT").performClick()
        rule.onNodeWithContentDescription("Value").performTextInput("9090")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/apps/a1/envs/PORT" } }
    }
}
