package mycelium.desktop

import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import mycelium.App
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import java.awt.Desktop
import java.awt.Toolkit
import java.awt.datatransfer.StringSelection
import java.net.InetAddress
import java.net.URI
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.attribute.PosixFilePermissions

@Serializable
private data class StoredConnection(val baseUrl: String, val token: String)

private object DesktopPlatform : Platform {
    private val configDir: Path = Path.of(System.getenv("XDG_CONFIG_HOME") ?: "${System.getProperty("user.home")}/.config", "mycelium")
    private val connectionFile: Path = configDir.resolve("desktop.json")

    override val signsInThroughTheBrowser = false

    override val originUrl: String? = null

    override fun loadConnection(): Connection? {
        if (!Files.exists(connectionFile)) {
            return null
        }

        val stored = runCatching { Json.decodeFromString<StoredConnection>(Files.readString(connectionFile)) }.getOrNull()

        return stored?.let { Connection(it.baseUrl, it.token) }
    }

    override fun saveConnection(connection: Connection?) {
        if (connection == null) {
            Files.deleteIfExists(connectionFile)
            return
        }

        Files.createDirectories(configDir)
        Files.deleteIfExists(connectionFile)
        Files.createFile(connectionFile)
        runCatching { Files.setPosixFilePermissions(connectionFile, PosixFilePermissions.fromString("rw-------")) }
        Files.writeString(connectionFile, Json.encodeToString(StoredConnection(connection.baseUrl, connection.token)))
    }

    override fun device(): DeviceInfo {
        val hostname = runCatching { InetAddress.getLocalHost().hostName }.getOrDefault("this computer")

        return DeviceInfo("Desktop app on $hostname", hostname, System.getProperty("os.name").lowercase(), System.getProperty("os.arch"))
    }

    override fun approveCode(): String? = null

    override fun currentPath(): String = "/"

    override fun navigate(url: String) = openUrl(url)

    override fun openUrl(url: String) {
        if (Desktop.isDesktopSupported() && Desktop.getDesktop().isSupported(Desktop.Action.BROWSE)) {
            Desktop.getDesktop().browse(URI(url))
        }
    }

    override fun copyToClipboard(text: String) {
        Toolkit.getDefaultToolkit().systemClipboard.setContents(StringSelection(text), null)
    }
}

fun main() = application {
    Window(
        onCloseRequest = ::exitApplication,
        title = "Mycelium",
        state = rememberWindowState(size = DpSize(1280.dp, 800.dp)),
    ) {
        App(DesktopPlatform)
    }
}
