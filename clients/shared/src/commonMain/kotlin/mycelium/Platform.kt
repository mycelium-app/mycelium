package mycelium

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.emptyFlow
import mycelium.state.PickedFile
import mycelium.ui.ButtonVariant
import mycelium.ui.Label
import mycelium.ui.MyButton
import mycelium.ui.Tokens

const val DefaultEventPageSize = 500

data class Connection(val baseUrl: String, val token: String)

data class PageReading(val script: String, val allowedUrl: (String) -> Boolean, val onRead: (String) -> Unit)

enum class CommandOutcome { Started, NotFound }

data class DeviceInfo(val label: String, val hostname: String, val os: String, val arch: String)

interface Platform {
    val signsInThroughTheBrowser: Boolean

    val readsPages: Boolean get() = false

    val originUrl: String?

    val signInCallback: String? get() = null

    val reducedMotion: Boolean get() = false

    val eventPageSize: Int get() = DefaultEventPageSize

    val resumes: Flow<Unit> get() = emptyFlow()

    fun loadConnection(): Connection?

    fun saveConnection(connection: Connection?)

    fun device(): DeviceInfo

    fun approveCode(): String?

    fun approveReturnTo(): String? = null

    fun closeWindow() {}

    fun currentPath(): String

    fun navigate(url: String)

    fun openUrl(url: String)

    fun revealPath(path: String) = Unit

    fun openSignIn(url: String, onCancelled: () -> Unit) = openUrl(url)

    fun copyToClipboard(text: String)

    @Composable
    fun WebContent(url: String, token: String?, modifier: Modifier, reading: PageReading? = null) {
        Column(modifier.fillMaxSize(), Arrangement.spacedBy(Tokens.Space.S3, Alignment.CenterVertically), Alignment.CenterHorizontally) {
            Label("This artefact can't be shown here.")
            MyButton(onClick = { openUrl(url) }, text = "Open in browser", variant = ButtonVariant.Outline)
        }
    }

    val showsLiveView: Boolean get() = false

    @Composable
    fun LiveView(url: String, viewOnly: Boolean, modifier: Modifier, onPointer: () -> Unit) {
        Column(modifier.fillMaxSize(), Arrangement.Center, Alignment.CenterHorizontally) {
            Label("The live view is available in the desktop app.")
        }
    }

    val opensIdes: Boolean get() = false

    fun hostname(): String? = null

    fun prepareSshHost(host: String, nodeId: String, token: String): String? = null

    fun runCommand(command: List<String>): CommandOutcome = CommandOutcome.NotFound

    fun timeZone(): String? = null

    fun localDateTime(epochMillis: Long): String? = null

    fun loadPreference(key: String): String? = null

    fun savePreference(key: String, value: String) = Unit

    fun systemAccent(dark: Boolean): Long? = null

    fun pickFiles(onPicked: (List<PickedFile>) -> Unit) = Unit

    fun clipboardFiles(): List<PickedFile> = emptyList()

    fun listenForFiles(listener: ((List<PickedFile>) -> Unit)?) = Unit

    fun windowFocused(): Boolean = false

    fun requestNotificationPermission() = Unit

    fun dictation(): Dictation? = null

    fun audioRecorder(): AudioRecorder? = null

    fun audioPlayer(): AudioPlayer? = null

    fun wakeListener(): WakeListener? = null

    val hearsWakePhrase: Boolean get() = wakeListener() != null

    val opensMicrophoneSettings: Boolean get() = false

    fun openMicrophoneSettings() = Unit

    fun notify(title: String, body: String, conversationId: String, sound: Boolean = true) = Unit

    fun chime() = Unit

    fun breakNotification(title: String, body: String) = Unit

    fun listenForNotificationOpens(listener: ((String) -> Unit)?) = Unit
}
