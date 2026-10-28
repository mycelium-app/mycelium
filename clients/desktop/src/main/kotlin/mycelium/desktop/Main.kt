package mycelium.desktop

import androidx.compose.foundation.draganddrop.dragAndDropTarget
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draganddrop.DragAndDropEvent
import androidx.compose.ui.draganddrop.DragAndDropTarget
import androidx.compose.ui.draganddrop.awtTransferable
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
import mycelium.AudioPlayer
import mycelium.AudioRecorder
import mycelium.CommandOutcome
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.PageReading
import mycelium.Platform
import mycelium.state.MaxAttachmentBytes
import mycelium.state.PickedFile
import mycelium.state.mediaTypeOf
import java.awt.FileDialog
import java.awt.Frame
import java.awt.datatransfer.DataFlavor
import java.awt.image.BufferedImage
import java.io.ByteArrayOutputStream
import java.io.File
import javax.imageio.ImageIO
import java.awt.Desktop
import java.awt.SystemTray
import java.awt.TrayIcon
import javax.sound.sampled.AudioFormat
import javax.sound.sampled.AudioSystem
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
        val opened = runCatching { ProcessBuilder("xdg-open", url).apply { environment().remove("LD_LIBRARY_PATH") }.start() }.isSuccess

        if (opened) {
            return
        }

        if (Desktop.isDesktopSupported() && Desktop.getDesktop().isSupported(Desktop.Action.BROWSE)) {
            Desktop.getDesktop().browse(URI(url))
        }
    }

    override val opensIdes = true

    override fun hostname(): String? = runCatching { InetAddress.getLocalHost().hostName }.getOrNull()

    override fun prepareSshHost(host: String, nodeId: String, token: String): String? {
        val connection = loadConnection() ?: return "Sign in before opening a folder on another node."
        val onPath = System.getenv("PATH").orEmpty().split(File.pathSeparator).any { Files.isExecutable(Path.of(it, "mycelium")) }

        if (!onPath) {
            return "The mycelium command was not found on this computer. Put it on your PATH so the IDE can reach the node over SSH."
        }

        val ssh = Path.of(System.getProperty("user.home"), ".ssh")
        val hosts = ssh.resolve("mycelium.d")
        val tokenFile = configDir.resolve("ssh-$host.token")

        return runCatching {
            Files.createDirectories(configDir)
            Files.deleteIfExists(tokenFile)
            Files.createFile(tokenFile, PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------")))
            Files.writeString(tokenFile, token)
            Files.createDirectories(hosts)
            Files.writeString(
                hosts.resolve("$host.conf"),
                "Host $host\n  HostName $host\n  User mycelium\n  StrictHostKeyChecking accept-new\n  UserKnownHostsFile ${ssh.resolve("mycelium_known_hosts")}\n  ProxyCommand mycelium ssh-proxy $nodeId --token-file $tokenFile --control-plane ${connection.baseUrl}\n",
            )
            includeMyceliumHosts(ssh)
            null
        }.getOrElse { "Mycelium could not write the SSH settings for this node: ${it.message}" }
    }

    private fun includeMyceliumHosts(ssh: Path) {
        val config = ssh.resolve("config")
        val include = "Include ${ssh.resolve("mycelium.d")}/*.conf"
        val existing = if (Files.exists(config)) Files.readString(config) else ""

        if (include !in existing) {
            Files.writeString(config, "$include\n\n$existing")
            runCatching { Files.setPosixFilePermissions(config, PosixFilePermissions.fromString("rw-------")) }
        }
    }

    override fun runCommand(command: List<String>): CommandOutcome =
        runCatching { ProcessBuilder(command).apply { environment().remove("LD_LIBRARY_PATH") }.start() }.fold({ CommandOutcome.Started }, { CommandOutcome.NotFound })

    override fun copyToClipboard(text: String) {
        Toolkit.getDefaultToolkit().systemClipboard.setContents(StringSelection(text), null)
    }

    override fun audioRecorder(): AudioRecorder = JavaSoundRecorder()

    override fun audioPlayer(): AudioPlayer = JavaSoundPlayer()

    override fun timeZone(): String? = java.time.ZoneId.systemDefault().id

    override fun localDateTime(epochMillis: Long): String? = java.time.LocalDateTime.ofInstant(java.time.Instant.ofEpochMilli(epochMillis), java.time.ZoneId.systemDefault()).format(java.time.format.DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm"))

    override fun loadPreference(key: String): String? = runCatching { Files.readString(configDir.resolve("$key.pref")).trim() }.getOrNull()

    override fun savePreference(key: String, value: String) {
        runCatching {
            Files.createDirectories(configDir)
            Files.writeString(configDir.resolve("$key.pref"), value)
        }
    }

    private var notificationOpener: ((String) -> Unit)? = null

    private var notifiedConversation: String? = null

    private val isLinux = System.getProperty("os.name").orEmpty().startsWith("Linux")

    private val trayIcon: TrayIcon? by lazy {
        runCatching {
            if (!SystemTray.isSupported()) {
                return@runCatching null
            }

            val image = BufferedImage(16, 16, BufferedImage.TYPE_INT_ARGB)
            val canvas = image.createGraphics()
            canvas.color = java.awt.Color(0x3d, 0x8b, 0x6e)
            canvas.fillOval(1, 1, 14, 14)
            canvas.dispose()

            val icon = TrayIcon(image, "Mycelium")
            icon.isImageAutoSize = true
            icon.addActionListener { notifiedConversation?.let { notificationOpener?.invoke(it) } }
            SystemTray.getSystemTray().add(icon)

            icon
        }.getOrNull()
    }

    override fun windowFocused(): Boolean = KeyboardFocusManager.getCurrentKeyboardFocusManager().activeWindow != null

    override fun notify(title: String, body: String, conversationId: String, sound: Boolean) {
        notifiedConversation = conversationId

        if (isLinux) {
            Thread { notifyOnLinux(title, body, conversationId) }.start()
        } else {
            trayIcon?.displayMessage(title, body, TrayIcon.MessageType.NONE)
        }
    }

    override fun breakNotification(title: String, body: String) = notify(title, body, "", false)

    private fun notifyOnLinux(title: String, body: String, conversationId: String) {
        val clicked = runCatching {
            val process = ProcessBuilder("notify-send", "--app-name=Mycelium", "--expire-time=5000", "--action=open=Open", "--wait", title, body).redirectErrorStream(true).start()
            val answer = process.inputStream.bufferedReader().readText().trim()

            process.waitFor() == 0 && answer == "open"
        }

        if (clicked.isFailure) {
            runCatching {
                ProcessBuilder("gdbus", "call", "--session", "--dest", "org.freedesktop.Notifications", "--object-path", "/org/freedesktop/Notifications", "--method", "org.freedesktop.Notifications.Notify", "Mycelium", "0", "", title, body, "[]", "{}", "5000").start().waitFor()
            }
        }

        if (clicked.getOrNull() == true) {
            notificationOpener?.invoke(conversationId)
        }
    }

    override val readsPages: Boolean get() = jcefAvailable

    @Composable
    override fun WebContent(url: String, token: String?, modifier: Modifier, reading: PageReading?) {
        if (jcefAvailable) {
            JcefWebContent(url, token, modifier, reading)
        } else {
            super.WebContent(url, token, modifier, reading)
        }
    }

    override val showsLiveView: Boolean get() = jcefAvailable

    @Composable
    override fun LiveView(url: String, viewOnly: Boolean, modifier: Modifier, onPointer: () -> Unit) {
        JcefWebContent(url, null, modifier, viewOnly = viewOnly, onPointer = onPointer)
    }

    override fun chime() {
        Thread {
            runCatching { playChime() }
        }.start()
    }

    override fun listenForNotificationOpens(listener: ((String) -> Unit)?) {
        notificationOpener = listener
    }

    private var fileListener: ((List<PickedFile>) -> Unit)? = null

    private fun read(file: File): PickedFile? = runCatching {
        val bytes = file.inputStream().use { it.readNBytes(MaxAttachmentBytes + 1) }

        PickedFile(file.name, mediaTypeOf(file.name, bytes), bytes)
    }.getOrNull()

    override fun pickFiles(onPicked: (List<PickedFile>) -> Unit) {
        val dialog = FileDialog(null as Frame?, "Attach files", FileDialog.LOAD)
        dialog.isMultipleMode = true
        dialog.isVisible = true

        onPicked(dialog.files.mapNotNull(::read))
    }

    override fun clipboardFiles(): List<PickedFile> = runCatching {
        val clipboard = Toolkit.getDefaultToolkit().systemClipboard

        when {
            clipboard.isDataFlavorAvailable(DataFlavor.javaFileListFlavor) ->
                (clipboard.getData(DataFlavor.javaFileListFlavor) as List<*>).filterIsInstance<File>().mapNotNull(::read)
            clipboard.isDataFlavorAvailable(DataFlavor.imageFlavor) -> {
                val image = clipboard.getData(DataFlavor.imageFlavor) as java.awt.Image
                val drawn = BufferedImage(image.getWidth(null), image.getHeight(null), BufferedImage.TYPE_INT_ARGB)
                drawn.graphics.drawImage(image, 0, 0, null)
                val encoded = ByteArrayOutputStream().also { ImageIO.write(drawn, "png", it) }.toByteArray()

                listOf(PickedFile("pasted-image.png", "image/png", encoded))
            }
            else -> emptyList()
        }
    }.getOrDefault(emptyList())

    override fun listenForFiles(listener: ((List<PickedFile>) -> Unit)?) {
        fileListener = listener
    }

    fun dropped(files: List<File>) {
        fileListener?.invoke(files.mapNotNull(::read))
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

private fun playChime() {
    val sampleRate = 44_100f
    val tones = listOf(659.25 to 0.125, 880.0 to 0.125)
    val format = AudioFormat(sampleRate, 16, 1, true, false)
    val samples = ByteArrayOutputStream()

    for ((frequency, seconds) in tones) {
        val count = (sampleRate * seconds).toInt()
        val fade = count / 5

        for (index in 0 until count) {
            val envelope = minOf(1.0, index.toDouble() / fade, (count - index).toDouble() / fade)
            val value = (Math.sin(2 * Math.PI * frequency * index / sampleRate) * envelope * 0.25 * Short.MAX_VALUE).toInt()
            samples.write(value and 0xff)
            samples.write((value shr 8) and 0xff)
        }
    }

    val line = AudioSystem.getSourceDataLine(format)
    line.open(format)
    line.start()
    line.write(samples.toByteArray(), 0, samples.size())
    line.drain()
    line.close()
}

@OptIn(ExperimentalComposeUiApi::class)
private object FileDrop : DragAndDropTarget {
    override fun onDrop(event: DragAndDropEvent): Boolean {
        val transferable = event.awtTransferable

        if (!transferable.isDataFlavorSupported(DataFlavor.javaFileListFlavor)) {
            return false
        }

        DesktopPlatform.dropped((transferable.getTransferData(DataFlavor.javaFileListFlavor) as List<*>).filterIsInstance<File>())

        return true
    }
}

private val ScaleSteps = listOf(0.8f, 0.9f, 1f, 1.1f, 1.25f, 1.5f, 1.75f, 2f)

private val ScaleCodes = setOf(
    AwtKeyEvent.VK_EQUALS, AwtKeyEvent.VK_PLUS, AwtKeyEvent.VK_ADD,
    AwtKeyEvent.VK_MINUS, AwtKeyEvent.VK_SUBTRACT, AwtKeyEvent.VK_0, AwtKeyEvent.VK_NUMPAD0,
)

@OptIn(ExperimentalComposeUiApi::class)
fun main() = application {
    LaunchedEffect(Unit) {
        Thread { jcefAvailable }.start()
    }

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
            Box(Modifier.fillMaxSize().dragAndDropTarget(shouldStartDragAndDrop = { true }, target = FileDrop)) {
                App(DesktopPlatform)
            }
        }
    }
}
