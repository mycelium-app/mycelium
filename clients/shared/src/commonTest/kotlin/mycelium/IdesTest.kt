package mycelium

import mycelium.state.Ide
import mycelium.state.IdeChoice
import mycelium.state.IdeSettings
import mycelium.state.absoluteInFolder
import mycelium.state.ideArguments
import mycelium.state.ideNotFound
import mycelium.state.ideSettings
import mycelium.state.runsOnThisComputer
import mycelium.state.sshHostAlias
import mycelium.state.stored
import kotlin.test.Test
import kotlin.test.assertEquals

class IdesTest {
    @Test
    fun lineNumbersFollowEachIdesOwnFlag() {
        assertEquals(listOf("/w/app/package.json:12"), ideArguments(Ide.Zed, "/w/app/package.json", 12, null))
        assertEquals(listOf("-g", "/w/app/package.json:12"), ideArguments(Ide.VsCode, "/w/app/package.json", 12, null))
        assertEquals(listOf("--line", "12", "/w/app/package.json"), ideArguments(Ide.JetBrains, "/w/app/package.json", 12, null))
        assertEquals(listOf("/w/app"), ideArguments(Ide.Zed, "/w/app", null, null))
    }

    @Test
    fun aRemoteFolderOpensThroughTheIdesSshRemote() {
        assertEquals(listOf("ssh://mycelium-atlas-box/w/app"), ideArguments(Ide.Zed, "/w/app", null, "mycelium-atlas-box"))
        assertEquals(listOf("--remote", "ssh-remote+mycelium-atlas-box", "-g", "/w/app/a.kt:3"), ideArguments(Ide.VsCode, "/w/app/a.kt", 3, "mycelium-atlas-box"))
    }

    @Test
    fun emptyCommandsFallBackToTheDefaultsAndJetBrainsIsNotOfferedForARemoteNode() {
        val settings = IdeSettings(listOf(IdeChoice("zed", true, "/opt/zed"), IdeChoice("code", true, "  "), IdeChoice("jetbrains", true, "")), default = "jetbrains")

        assertEquals("/opt/zed", settings.commandFor(Ide.Zed))
        assertEquals("code", settings.commandFor(Ide.VsCode))
        assertEquals(listOf(Ide.Zed, Ide.VsCode, Ide.JetBrains), settings.offered(remote = false))
        assertEquals(listOf(Ide.Zed, Ide.VsCode), settings.offered(remote = true))
        assertEquals(Ide.JetBrains, settings.main(remote = false))
        assertEquals(Ide.Zed, settings.main(remote = true))
    }

    @Test
    fun settingsRoundTripAndAnUnreadableStoredValueMeansTheDefaults() {
        val changed = IdeSettings(listOf(IdeChoice("zed", false), IdeChoice("code", true, "codium")), default = "code")

        assertEquals(changed, ideSettings(changed.stored()))
        assertEquals(IdeSettings(), ideSettings("not json"))
        assertEquals(IdeSettings(), ideSettings(null))
    }

    @Test
    fun theNotFoundMessageNamesTheCommandAndWhereToSetIt() {
        assertEquals("Zed was not found at zed. Set its command in Settings, IDEs.", ideNotFound(Ide.Zed, "zed"))
    }

    @Test
    fun aNodeOnThisComputerIsRecognisedByItsHostname() {
        assertEquals(true, runsOnThisComputer("laptop.local", "Laptop"))
        assertEquals(false, runsOnThisComputer("atlas-box", "laptop"))
        assertEquals(false, runsOnThisComputer("atlas-box", null))
    }

    @Test
    fun theSshHostAliasIsTheNodeNameMadeSafeForSshConfig() {
        assertEquals("mycelium-atlas-box", sshHostAlias("Atlas Box"))
        assertEquals("mycelium-a-b", sshHostAlias("  a/b!"))
    }

    @Test
    fun relativeFilesAreResolvedAgainstTheConversationFolder() {
        assertEquals("/w/app/src/a.kt", absoluteInFolder("/w/app/", "src/a.kt"))
        assertEquals("/etc/hosts", absoluteInFolder("/w/app", "/etc/hosts"))
    }
}
