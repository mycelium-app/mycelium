package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
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
import java.awt.KeyboardFocusManager
import java.awt.Toolkit
import java.awt.event.KeyEvent as AwtKeyEvent
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
    private val scaleFile: Path = configDir.resolve("scale")

    val scale = mutableStateOf(runCatching { Files.readString(scaleFile).trim().toFloat() }.getOrDefault(1f))

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
        val opened = runCatching { ProcessBuilder("xdg-open", url).start() }.isSuccess

        if (opened) {
            return
        }

        if (Desktop.isDesktopSupported() && Desktop.getDesktop().isSupported(Desktop.Action.BROWSE)) {
            Desktop.getDesktop().browse(URI(url))
        }
    }

    override fun copyToClipboard(text: String) {
        Toolkit.getDefaultToolkit().systemClipboard.setContents(StringSelection(text), null)
    }

    fun stepScale(keyCode: Int): Boolean {
        val current = scale.value
        val next = when (keyCode) {
            AwtKeyEvent.VK_EQUALS, AwtKeyEvent.VK_PLUS, AwtKeyEvent.VK_ADD -> ScaleSteps.firstOrNull { it > current + 0.01f }
            AwtKeyEvent.VK_MINUS, AwtKeyEvent.VK_SUBTRACT -> ScaleSteps.lastOrNull { it < current - 0.01f }
            AwtKeyEvent.VK_0, AwtKeyEvent.VK_NUMPAD0 -> 1f
            else -> return false
        }

        if (next != null) {
            scale.value = next
            runCatching {
                Files.createDirectories(configDir)
                Files.writeString(scaleFile, next.toString())
            }
        }

        return true
    }
}

private val ScaleSteps = listOf(0.8f, 0.9f, 1f, 1.1f, 1.25f, 1.5f, 1.75f, 2f)

private val ScaleCodes = setOf(
    AwtKeyEvent.VK_EQUALS, AwtKeyEvent.VK_PLUS, AwtKeyEvent.VK_ADD,
    AwtKeyEvent.VK_MINUS, AwtKeyEvent.VK_SUBTRACT, AwtKeyEvent.VK_0, AwtKeyEvent.VK_NUMPAD0,
)

fun main() = application {
    remember {
        KeyboardFocusManager.getCurrentKeyboardFocusManager().addKeyEventDispatcher { event ->
            if (!event.isControlDown) {
                return@addKeyEventDispatcher false
            }

            when (event.id) {
                AwtKeyEvent.KEY_PRESSED -> DesktopPlatform.stepScale(event.keyCode)
                AwtKeyEvent.KEY_TYPED -> event.keyChar in "=+-_0"
                AwtKeyEvent.KEY_RELEASED -> event.keyCode in ScaleCodes
                else -> false
            }
        }
    }

    Window(
        onCloseRequest = ::exitApplication,
        title = "Mycelium",
        state = rememberWindowState(size = DpSize(1280.dp, 800.dp)),
    ) {
        val density = LocalDensity.current

        CompositionLocalProvider(LocalDensity provides Density(density.density * DesktopPlatform.scale.value, density.fontScale)) {
            App(DesktopPlatform)
        }
    }
}
