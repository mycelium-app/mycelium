import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.window.ComposeViewport
import mycelium.App
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.state.PickedFile
import mycelium.state.decodeBase64
import mycelium.state.mediaTypeOf

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

private object WebPlatform : Platform {
    private var fileListener: ((List<PickedFile>) -> Unit)? = null

    init {
        listenForPastedAndDroppedFiles { name, type, data -> received(name, type, data, fileListener) }
    }

    private fun received(name: JsString, type: JsString, data: JsString, listener: ((List<PickedFile>) -> Unit)?) {
        val bytes = decodeBase64(data.toString()) ?: return
        val fileName = name.toString().ifBlank { "pasted-file" }

        listener?.invoke(listOf(PickedFile(fileName, mediaTypeOf(fileName, bytes, type.toString()), bytes)))
    }

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

    override fun currentPath(): String = pagePath() + pageQuery()

    override fun navigate(url: String) = goTo(url)

    override fun openUrl(url: String) = openTab(url)

    override fun copyToClipboard(text: String) = writeClipboard(text)

    override fun loadPreference(key: String): String? = readStorage(key)

    override fun savePreference(key: String, value: String) = writeStorage(key, value)
}

@JsFun("() => { const status = document.getElementById('status'); if (status) status.remove(); }")
private external fun hideLoading()

@OptIn(ExperimentalComposeUiApi::class)
fun main() {
    ComposeViewport {
        LaunchedEffect(Unit) { hideLoading() }

        App(WebPlatform)
    }
}
