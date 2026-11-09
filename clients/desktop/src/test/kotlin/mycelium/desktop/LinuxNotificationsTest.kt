package mycelium.desktop

import java.io.IOException
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

private class FakeDesktop(private val answer: (List<String>) -> CommandResult) {
    val commands = mutableListOf<List<String>>()
    val opened = mutableListOf<String>()

    val notifications = LinuxNotifications(
        run = { command ->
            commands += command
            answer(command)
        },
        inBackground = { it() },
        open = { opened += it },
    )
}

class LinuxNotificationsTest {
    @Test
    fun clickingTheBodyOpensThatConversation() {
        val desktop = FakeDesktop { CommandResult(0, "default") }

        desktop.notifications.show("Agent finished", "Done", "conversation-a")

        assertEquals(listOf("conversation-a"), desktop.opened)
    }

    @Test
    fun choosingOpenOpensThatConversation() {
        val desktop = FakeDesktop { CommandResult(0, "open") }

        desktop.notifications.show("Agent finished", "Done", "conversation-a")

        assertEquals(listOf("conversation-a"), desktop.opened)
    }

    @Test
    fun dismissingTheNotificationOpensNothing() {
        val desktop = FakeDesktop { CommandResult(0, "") }

        desktop.notifications.show("Agent finished", "Done", "conversation-a")

        assertEquals(emptyList(), desktop.opened)
    }

    @Test
    fun aBreakNotificationOffersNothingToOpen() {
        val desktop = FakeDesktop { CommandResult(0, "default") }

        desktop.notifications.show("Time for a break", "You have been at it for 50 minutes.", null)

        assertEquals(emptyList(), desktop.opened)
        assertFalse(desktop.commands.single().any { it.startsWith("--action") })
    }

    @Test
    fun twoNotificationsEachOpenTheirOwnConversation() {
        val desktop = FakeDesktop { command -> CommandResult(0, if ("First" in command) "default" else "open") }

        desktop.notifications.show("First", "One", "conversation-a")
        desktop.notifications.show("Second", "Two", "conversation-b")

        assertEquals(listOf("conversation-a", "conversation-b"), desktop.opened)
    }

    @Test
    fun aMissingNotifySendFallsBackToGdbus() {
        val desktop = FakeDesktop { command -> if (command.first() == "notify-send") throw IOException("not found") else CommandResult(0, "") }

        desktop.notifications.show("Agent finished", "Done", "conversation-a")

        assertEquals(listOf("notify-send", "gdbus"), desktop.commands.map { it.first() })
        assertTrue("Agent finished" in desktop.commands.last())
    }

    @Test
    fun aFailingNotifySendFallsBackToGdbus() {
        val desktop = FakeDesktop { command -> if (command.first() == "notify-send") CommandResult(1, "Unknown option --wait") else CommandResult(0, "") }

        desktop.notifications.show("Agent finished", "Done", "conversation-a")

        assertEquals(listOf("notify-send", "gdbus"), desktop.commands.map { it.first() })
        assertEquals(emptyList(), desktop.opened)
    }
}
