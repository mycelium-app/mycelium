package mycelium.desktop

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.awt.SwingPanel
import com.jetbrains.cef.JCefAppConfig
import org.cef.CefApp
import org.cef.CefSettings
import org.cef.CefClient
import org.cef.browser.CefBrowser
import org.cef.network.CefCookie
import org.cef.network.CefCookieManager
import java.net.URI
import java.util.Date

private const val SessionCookie = "mycelium_client"

private object Jcef {
    val app: CefApp by lazy {
        val config = JCefAppConfig.getInstance()
        val args = config.appArgs

        CefApp.startup(args)

        val getInstance = CefApp::class.java.methods.filter { it.name == "getInstance" }
        val withSettings = getInstance.firstOrNull { it.parameterTypes.toList() == listOf(Array<String>::class.java, CefSettings::class.java) }
        val withCacheFolder = getInstance.firstOrNull { it.parameterTypes.toList() == listOf(Array<String>::class.java, CefSettings::class.java, java.io.File::class.java) }

        when {
            withSettings != null -> withSettings.invoke(null, args, config.cefSettings) as CefApp
            withCacheFolder != null -> withCacheFolder.invoke(null, args, config.cefSettings, null) as CefApp
            else -> CefApp.getInstance(config.cefSettings)
        }
    }

    fun createClient(): CefClient = app.createClient()

    fun setSessionCookie(url: String, token: String) {
        val uri = URI(url)
        val host = uri.host ?: return
        val now = Date()
        val cookie = CefCookie(SessionCookie, token, host, "/", uri.scheme == "https", true, now, now, false, now)

        CefCookieManager.getGlobalManager().setCookie(url, cookie)
    }
}

val jcefAvailable: Boolean by lazy {
    runCatching { Jcef.app }
        .onFailure { cause -> System.err.println("The built-in browser could not start, so artefacts open in your browser instead: $cause") }
        .isSuccess
}

@Composable
fun JcefWebContent(url: String, token: String?, modifier: Modifier) {
    val client = remember { Jcef.createClient() }
    val browser: CefBrowser = remember { client.createBrowser("about:blank", false, false) }

    DisposableEffect(Unit) {
        onDispose {
            browser.close(true)
            client.dispose()
        }
    }

    LaunchedEffect(url, token) {
        if (token != null) {
            Jcef.setSessionCookie(url, token)
        }

        browser.loadURL(url)
    }

    SwingPanel(factory = { browser.uiComponent }, modifier = modifier)
}
