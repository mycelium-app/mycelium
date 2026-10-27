package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.CommandOutcome
import mycelium.Platform
import mycelium.api.ApiException
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.NodeSummary
import mycelium.state.Ide
import mycelium.state.IdePreference
import mycelium.state.IdeSettings
import mycelium.state.ideSettings
import mycelium.state.openInIde
import mycelium.ui.IdesSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

private class IdePlatform(val thisHostname: String, var outcome: CommandOutcome = CommandOutcome.Started) : Platform by PlatformStub {
    val preferences = mutableMapOf<String, String>()
    val commands = mutableListOf<List<String>>()
    val sshHosts = mutableListOf<Triple<String, String, String>>()

    override val opensIdes = true

    override fun hostname(): String = thisHostname

    override fun loadPreference(key: String): String? = preferences[key]

    override fun savePreference(key: String, value: String) {
        preferences[key] = value
    }

    override fun prepareSshHost(host: String, nodeId: String, token: String): String? {
        sshHosts += Triple(host, nodeId, token)

        return null
    }

    override fun runCommand(command: List<String>): CommandOutcome {
        commands += command

        return outcome
    }
}

private fun node(name: String, hostname: String) = MyceliumJson.decodeFromString<NodeSummary>("""{"id":"n1","name":"$name","hostname":"$hostname","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false}""")

private val api = MyceliumApi("https://mycelium.test", "token", MockEngine { respond("""{"token":"myc_ssh_secret","expires_at":"2026-10-27T12:10:00Z"}""", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json")) })

class IdesTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun aFolderOnThisComputerOpensDirectlyWithTheLineFlag() = runBlocking {
        val platform = IdePlatform("laptop")

        openInIde(api, platform, node("laptop", "laptop.local"), Ide.VsCode, IdeSettings(), "/w/app/a.kt", 7)

        assertEquals(listOf(listOf("code", "-g", "/w/app/a.kt:7")), platform.commands)
        assertEquals(emptyList(), platform.sshHosts)
    }

    @Test
    fun aFolderOnAnotherNodeMintsATokenPreparesTheSshHostAndOpensItsRemote() = runBlocking {
        val platform = IdePlatform("laptop")

        openInIde(api, platform, node("Atlas Box", "atlas"), Ide.Zed, IdeSettings(), "/w/app", null)

        assertEquals(listOf(Triple("mycelium-atlas-box", "n1", "myc_ssh_secret")), platform.sshHosts)
        assertEquals(listOf(listOf("zed", "ssh://mycelium-atlas-box/w/app")), platform.commands)
    }

    @Test
    fun aMissingIdeSaysWhereToSetItsCommand() {
        val platform = IdePlatform("laptop", CommandOutcome.NotFound)

        val failure = assertFailsWith<ApiException> { runBlocking { openInIde(api, platform, node("laptop", "laptop"), Ide.Zed, IdeSettings(), "/w/app", null) } }

        assertEquals("Zed was not found at zed. Set its command in Settings, IDEs.", failure.message)
    }

    @Test
    fun savingKeepsTheTickedIdesAndCancelDropsTheDraft() {
        val platform = IdePlatform("laptop")
        rule.setContent { MyceliumTheme(true) { IdesSettings(platform) } }

        rule.onNodeWithContentDescription("Offer VS Code").performClick()
        rule.onNodeWithText("Cancel").performClick()
        rule.onNodeWithText("Save").performClick()

        assertEquals(true, ideSettings(platform.preferences[IdePreference]).choiceFor(Ide.VsCode).offered)

        rule.onNodeWithContentDescription("Offer VS Code").performClick()
        rule.onNodeWithText("Save").performClick()

        assertEquals(false, ideSettings(platform.preferences[IdePreference]).choiceFor(Ide.VsCode).offered)
    }
}
