package mycelium

import mycelium.api.AppDeployment
import mycelium.api.AppEnv
import mycelium.api.AppSummary
import mycelium.api.ChatToolServer
import mycelium.state.addressLabel
import mycelium.state.appMatches
import mycelium.state.appStatusWord
import mycelium.state.appsLinkVisible
import mycelium.state.commitLine
import mycelium.state.deployCallOf
import mycelium.state.envNameProblem
import mycelium.state.envNotice
import mycelium.state.envShownValue
import mycelium.state.envValueProblem
import mycelium.state.filteredLogLines
import mycelium.state.wildcardLabel
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

private fun server(kind: String) = ChatToolServer(id = kind, name = kind, kind = kind, url = "https://x.example.com", createdAt = "2026-10-27T09:00:00Z")

private fun env(key: String, secret: Boolean = false, stale: Boolean = false, value: String? = "1") = AppEnv(key, secret = secret, value = if (secret) null else value, stale = stale)

private const val DeployOutput = """{"app_uuid":"a1","app_name":"harbour-status","deployment_uuid":"d1","address":"https://harbour-status.apps.example.com"}"""

class AppsStateTest {
    @Test
    fun theAppsLinkIsVisibleOnlyWhileACoolifyAccountExists() {
        assertEquals(false, appsLinkVisible(emptyList()))
        assertEquals(false, appsLinkVisible(listOf(server("paperless"), server("tandoor"))))
        assertEquals(true, appsLinkVisible(listOf(server("paperless"), server("coolify"))))
    }

    @Test
    fun statusesAreNamedInPlainWords() {
        assertEquals(listOf("Running", "Deploying", "Failed", "Stopped"), listOf("running", "deploying", "failed", "stopped").map(::appStatusWord))
    }

    @Test
    fun theEnvironmentNoticeNamesTheVariableChangedAfterTheLastDeploy() {
        assertNull(envNotice(listOf(env("PORT"))))
        assertEquals("DATABASE_URL changed after the last deploy. Redeploy to use it.", envNotice(listOf(env("PORT"), env("DATABASE_URL", secret = true, stale = true))))
        assertEquals("A and 1 more changed after the last deploy. Redeploy to use them.", envNotice(listOf(env("A", stale = true), env("B", stale = true))))
    }

    @Test
    fun anEmptyValueNamesTheVariable() {
        assertEquals("Enter a value for DATABASE_URL", envValueProblem("DATABASE_URL", ""))
        assertNull(envValueProblem("DATABASE_URL", "x"))
    }

    @Test
    fun aNewVariableNeedsAValidUnusedName() {
        val existing = listOf(env("PORT"))

        assertEquals("Enter a name for the variable", envNameProblem(" ", existing))
        assertEquals("Enter a different name. PORT already exists.", envNameProblem("PORT", existing))
        assertNull(envNameProblem("STATUS_TITLE", existing))
    }

    @Test
    fun secretValuesAreNeverShownEvenWhenRevealed() {
        assertEquals("••••••••••", envShownValue(env("PORT"), revealed = false))
        assertEquals("1", envShownValue(env("PORT"), revealed = true))
        assertEquals("••••••••••", envShownValue(env("TOKEN", secret = true), revealed = true))
    }

    @Test
    fun logSearchKeepsMatchingLinesIgnoringCase() {
        val lines = listOf("14:31:12 ERROR Could not reach the backup database", "14:31:42 INFO Reconnected")

        assertEquals(listOf(lines[0]), filteredLogLines(lines, " error "))
        assertEquals(lines, filteredLogLines(lines, ""))
    }

    @Test
    fun aDeployCallIsRecognisedFromTheToolNameAndTheDeploymentInItsOutput() {
        assertEquals("d1", deployCallOf("deploy", DeployOutput)?.deploymentUuid)
        assertEquals("harbour-status", deployCallOf("coolify__create_app", DeployOutput)?.appName)
        assertNull(deployCallOf("list_apps", DeployOutput))
        assertNull(deployCallOf("deploy", """{"app_uuid":"a1"}"""))
        assertNull(deployCallOf("deploy", "Deployment queued"))
        assertNull(deployCallOf("deploy", null))
    }

    @Test
    fun addressesAndWildcardsAreShownWithoutTheScheme() {
        assertEquals("harbour-status.apps.example.com", addressLabel("https://harbour-status.apps.example.com/"))
        assertEquals("*.apps.example.com", wildcardLabel("https://apps.example.com"))
        assertEquals("*.apps.example.com", wildcardLabel("*.apps.example.com"))
        assertNull(wildcardLabel(null))
    }

    @Test
    fun appsAreSearchedByNameAddressAndRepository() {
        val app = AppSummary("a1", "harbour-status", "running", "https://harbour-status.apps.example.com", repository = "git.example.com/sam/harbour")

        assertEquals(true, appMatches(app, "STATUS"))
        assertEquals(true, appMatches(app, "sam/harbour"))
        assertEquals(false, appMatches(app, "ledger"))
    }

    @Test
    fun aCommitIsShownAsItsShortShaAndMessage() {
        assertEquals("4f2c1a9 Add the uptime chart", commitLine(AppDeployment("d1", "2026-10-28T14:02:00Z", commit = "4f2c1a9d3e", commitMessage = "Add the uptime chart")))
    }
}
