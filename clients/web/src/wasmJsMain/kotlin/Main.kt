import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.window.ComposeViewport
import mycelium.App
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Dictation
import mycelium.DictationProblem
import mycelium.Platform
import mycelium.state.NotifyFinishedPreference
import mycelium.state.PickedFile
import mycelium.state.decodeBase64
import mycelium.state.mediaTypeOf

@JsFun("() => Intl.DateTimeFormat().resolvedOptions().timeZone || ''")
private external fun browserTimeZone(): String

@JsFun("(millis) => { const at = new Date(millis); const two = (value) => String(value).padStart(2, '0'); return at.getFullYear() + '-' + two(at.getMonth() + 1) + '-' + two(at.getDate()) + 'T' + two(at.getHours()) + ':' + two(at.getMinutes()) }")
private external fun browserDateTime(millis: Double): String

@JsFun("() => window.location.origin")
private external fun pageOrigin(): String

@JsFun("() => window.location.pathname")
private external fun pagePath(): String

@JsFun("() => window.location.search")
private external fun pageQuery(): String

@JsFun("(url) => { window.location.href = url; }")
private external fun goTo(url: String)

@JsFun("(url) => { window.open(url, '_blank', 'noopener'); }")
private external fun openTab(url: String)

@JsFun("() => { window.close(); }")
private external fun closeThisWindow()

@JsFun("(text) => { navigator.clipboard.writeText(text); }")
private external fun writeClipboard(text: String)

@JsFun("() => window.matchMedia('(prefers-reduced-motion: reduce)').matches")
private external fun prefersReducedMotion(): Boolean

@JsFun("(key) => { try { return window.localStorage.getItem(key); } catch (e) { return null; } }")
private external fun readStorage(key: String): String?

@JsFun("(key, value) => { try { window.localStorage.setItem(key, value); } catch (e) {} }")
private external fun writeStorage(key: String, value: String)

@JsFun("""(onFile) => {
    const read = (files) => {
        for (const file of files) {
            const reader = new FileReader();
            reader.onload = () => onFile(file.name, file.type, String(reader.result).split(',', 2)[1] || '');
            reader.readAsDataURL(file);
        }
    };
    document.addEventListener('paste', (event) => {
        const files = Array.from(event.clipboardData ? event.clipboardData.files : []);
        if (files.length > 0) { event.preventDefault(); read(files); }
    });
    document.addEventListener('dragover', (event) => {
        if (event.dataTransfer && Array.from(event.dataTransfer.types).includes('Files')) { event.preventDefault(); }
    });
    document.addEventListener('drop', (event) => {
        if (event.dataTransfer && event.dataTransfer.files.length > 0) { event.preventDefault(); read(Array.from(event.dataTransfer.files)); }
    });
}""")
private external fun listenForPastedAndDroppedFiles(onFile: (JsString, JsString, JsString) -> Unit)

@JsFun("""(onFile) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.multiple = true;
    input.onchange = () => {
        for (const file of Array.from(input.files)) {
            const reader = new FileReader();
            reader.onload = () => onFile(file.name, file.type, String(reader.result).split(',', 2)[1] || '');
            reader.readAsDataURL(file);
        }
    };
    input.click();
}""")
private external fun chooseFiles(onFile: (JsString, JsString, JsString) -> Unit)

@JsFun("() => document.hasFocus()")
private external fun pageFocused(): Boolean

@JsFun("() => { if (typeof Notification !== 'undefined' && Notification.permission === 'default') { Notification.requestPermission(); } }")
private external fun askNotificationPermission()

@JsFun("() => { if (typeof Notification !== 'undefined' && Notification.permission === 'default') { document.addEventListener('pointerdown', () => Notification.requestPermission(), { once: true }); } }")
private external fun askNotificationPermissionOnFirstClick()

@JsFun("""(title, body, id, onOpen) => {
    if (typeof Notification === 'undefined' || Notification.permission !== 'granted') { return; }
    const shown = new Notification(title, { body, tag: id });
    shown.onclick = () => { window.focus(); onOpen(id); shown.close(); };
}""")
private external fun showNotification(title: String, body: String, id: String, onOpen: (JsString) -> Unit)

@JsFun("""() => {
    const Context = window.AudioContext || window.webkitAudioContext;
    if (!Context) { return; }
    const audio = new Context();
    const start = audio.currentTime;
    [[659.25, 0], [880, 0.125]].forEach(([frequency, offset]) => {
        const oscillator = audio.createOscillator();
        const gain = audio.createGain();
        oscillator.frequency.value = frequency;
        gain.gain.setValueAtTime(0, start + offset);
        gain.gain.linearRampToValueAtTime(0.25, start + offset + 0.025);
        gain.gain.linearRampToValueAtTime(0, start + offset + 0.125);
        oscillator.connect(gain).connect(audio.destination);
        oscillator.start(start + offset);
        oscillator.stop(start + offset + 0.125);
    });
    setTimeout(() => audio.close(), 500);
}""")
private external fun playChime()

@JsFun("() => !!(window.SpeechRecognition || window.webkitSpeechRecognition)")
private external fun speechRecognitionExists(): Boolean

@JsFun("""(onHeard, onEnded) => {
    const Recognition = window.SpeechRecognition || window.webkitSpeechRecognition;
    const recognition = new Recognition();
    let failure = '';
    recognition.continuous = true;
    recognition.interimResults = false;
    recognition.lang = navigator.language;
    recognition.onresult = (event) => {
        for (let index = event.resultIndex; index < event.results.length; index++) {
            if (event.results[index].isFinal) { onHeard(event.results[index][0].transcript); }
        }
    };
    recognition.onerror = (event) => { failure = event.error || 'failed'; };
    recognition.onend = () => onEnded(failure);
    recognition.start();
    return recognition;
}""")
private external fun startSpeechRecognition(onHeard: (JsString) -> Unit, onEnded: (JsString) -> Unit): JsAny

@JsFun("(recognition) => { recognition.stop(); }")
private external fun stopSpeechRecognition(recognition: JsAny)

private class WebDictation : Dictation {
    private var recognition: JsAny? = null

    override fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) {
        recognition = startSpeechRecognition({ heard -> onHeard(heard.toString()) }) { failure ->
            recognition = null
            onStopped(
                when (failure.toString()) {
                    "", "no-speech", "aborted" -> null
                    "not-allowed", "service-not-allowed" -> DictationProblem.Denied
                    "audio-capture" -> DictationProblem.NoMicrophone
                    else -> DictationProblem.Failed("Dictation stopped. Try again.")
                },
            )
        }
    }

    override fun stop() {
        recognition?.let { stopSpeechRecognition(it) }
    }
}

private object WebPlatform : Platform {
    private var notificationOpener: ((String) -> Unit)? = null

    private var fileListener: ((List<PickedFile>) -> Unit)? = null

    init {
        listenForPastedAndDroppedFiles { name, type, data -> received(name, type, data, fileListener) }
    }

    private fun received(name: JsString, type: JsString, data: JsString, listener: ((List<PickedFile>) -> Unit)?) {
        val bytes = decodeBase64(data.toString()) ?: return
        val fileName = name.toString().ifBlank { "pasted-file" }

        listener?.invoke(listOf(PickedFile(fileName, mediaTypeOf(fileName, bytes, type.toString()), bytes)))
    }

    override fun dictation(): Dictation? = if (speechRecognitionExists()) WebDictation() else null

    override fun pickFiles(onPicked: (List<PickedFile>) -> Unit) {
        chooseFiles { name, type, data -> received(name, type, data, onPicked) }
    }

    override fun listenForFiles(listener: ((List<PickedFile>) -> Unit)?) {
        fileListener = listener
    }

    override val signsInThroughTheBrowser = true

    override val originUrl: String = pageOrigin()

    override val reducedMotion: Boolean = prefersReducedMotion()

    override fun loadConnection(): Connection? = null

    override fun saveConnection(connection: Connection?) = Unit

    override fun device(): DeviceInfo = DeviceInfo("Browser", "browser", "web", "wasm")

    override fun approveCode(): String? {
        if (pagePath() != "/approve") {
            return null
        }

        return pageQuery().removePrefix("?").split("&").firstNotNullOfOrNull { pair ->
            pair.takeIf { it.startsWith("code=") }?.removePrefix("code=")?.ifBlank { null }
        }
    }

    override fun approveReturnTo(): String? = pageQuery().removePrefix("?").split("&").firstNotNullOfOrNull { pair ->
        pair.takeIf { it.startsWith("return_to=") }?.removePrefix("return_to=")
    }

    override fun currentPath(): String = pagePath() + pageQuery()

    override fun navigate(url: String) = goTo(url)

    override fun openUrl(url: String) = openTab(url)

    override fun closeWindow() = closeThisWindow()

    override fun copyToClipboard(text: String) = writeClipboard(text)

    @Composable
    override fun WebContent(url: String, token: String?, modifier: Modifier) = IframeWebContent(url, modifier)

    override fun timeZone(): String? = browserTimeZone().ifBlank { null }

    override fun localDateTime(epochMillis: Long): String? = browserDateTime(epochMillis.toDouble())

    override fun windowFocused(): Boolean = pageFocused()

    override fun requestNotificationPermission() = askNotificationPermission()

    override fun notify(title: String, body: String, conversationId: String, sound: Boolean) {
        showNotification(title, body, conversationId) { id -> notificationOpener?.invoke(id.toString()) }
    }

    override fun chime() = playChime()

    override fun listenForNotificationOpens(listener: ((String) -> Unit)?) {
        notificationOpener = listener
    }

    override fun loadPreference(key: String): String? = readStorage(key)

    override fun savePreference(key: String, value: String) = writeStorage(key, value)
}

@JsFun("() => { const status = document.getElementById('status'); if (status) status.remove(); }")
private external fun hideLoading()

@OptIn(ExperimentalComposeUiApi::class)
fun main() {
    if (WebPlatform.loadPreference(NotifyFinishedPreference) != false.toString()) {
        askNotificationPermissionOnFirstClick()
    }

    ComposeViewport {
        LaunchedEffect(Unit) { hideLoading() }

        App(WebPlatform)
    }
}
