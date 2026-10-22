import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.window.ComposeViewport
import mycelium.App
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform

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

private object WebPlatform : Platform {
    override val signsInThroughTheBrowser = true

    override val originUrl: String = pageOrigin()

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
}

@OptIn(ExperimentalComposeUiApi::class)
fun main() {
    ComposeViewport {
        App(WebPlatform)
    }
}
