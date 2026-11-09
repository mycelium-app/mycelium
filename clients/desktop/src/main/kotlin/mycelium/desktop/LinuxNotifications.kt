package mycelium.desktop

data class CommandResult(val exitCode: Int, val output: String)

private val openingActions = setOf("default", "open")

class LinuxNotifications(
    private val run: (List<String>) -> CommandResult = { command ->
        val process = ProcessBuilder(command).redirectError(ProcessBuilder.Redirect.DISCARD).start()
        val output = process.inputStream.bufferedReader().readText()

        CommandResult(process.waitFor(), output)
    },
    private val inBackground: (() -> Unit) -> Unit = { Thread(it).start() },
    private val open: (String) -> Unit,
) {
    fun show(title: String, body: String, conversationId: String?) = inBackground {
        val actions = if (conversationId == null) emptyList() else listOf("--action=default=Open", "--action=open=Open", "--wait")
        val shown = runCatching { run(listOf("notify-send", "--app-name=Mycelium", "--expire-time=5000") + actions + listOf(title, body)) }.getOrNull()

        if (shown == null || shown.exitCode != 0) {
            runCatching {
                run(listOf("gdbus", "call", "--session", "--dest", "org.freedesktop.Notifications", "--object-path", "/org/freedesktop/Notifications", "--method", "org.freedesktop.Notifications.Notify", "Mycelium", "0", "", title, body, "[]", "{}", "5000"))
            }

            return@inBackground
        }

        if (conversationId != null && shown.output.trim() in openingActions) {
            open(conversationId)
        }
    }
}
