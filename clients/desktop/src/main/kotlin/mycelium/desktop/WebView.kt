package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.awt.SwingPanel
import com.jetbrains.cef.JCefAppConfig
import org.cef.CefApp
import org.cef.CefSettings
import org.cef.CefClient
import org.cef.browser.CefBrowser
import org.cef.browser.CefRendering
import org.cef.network.CefCookie
import org.cef.network.CefCookieManager
import java.net.URI
import java.util.Date
import kotlinx.coroutines.delay

private const val SessionCookie = "mycelium_client"

private fun cacheFolder(): String {
    val base = System.getenv("XDG_CACHE_HOME")?.takeIf { it.isNotBlank() } ?: (System.getProperty("user.home") + "/.cache")
    val folder = java.io.File(base, "mycelium/jcef")

    folder.mkdirs()

    return folder.absolutePath
}

private object Jcef {
    val app: CefApp by lazy {
        val config = JCefAppConfig.getInstance()
        val args = config.appArgs
        val settings = config.cefSettings
        val cache = cacheFolder()

        settings.cache_path = cache
        CefSettings::class.java.fields.firstOrNull { it.name == "root_cache_path" }?.set(settings, cache)

        CefApp.startup(args)

        val getInstance = CefApp::class.java.methods.filter { it.name == "getInstance" }
        val withSettings = getInstance.firstOrNull { it.parameterTypes.toList() == listOf(Array<String>::class.java, CefSettings::class.java) }
        val withCacheFolder = getInstance.firstOrNull { it.parameterTypes.toList() == listOf(Array<String>::class.java, CefSettings::class.java, java.io.File::class.java) }

        when {
            withSettings != null -> withSettings.invoke(null, args, settings) as CefApp
            withCacheFolder != null -> withCacheFolder.invoke(null, args, settings, null) as CefApp
            else -> CefApp.getInstance(settings)
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
    var ready by remember { mutableStateOf(CefApp.getState() == CefApp.CefAppState.INITIALIZED) }

    LaunchedEffect(Unit) {
        while (!ready && CefApp.getState() != CefApp.CefAppState.TERMINATED) {
            delay(100)
            ready = CefApp.getState() == CefApp.CefAppState.INITIALIZED
        }
    }

    if (!ready) {
        Box(modifier)
        return
    }

    val client = remember { Jcef.createClient() }
    val panel = remember { OffscreenBrowserPanel() }
    val browser: CefBrowser = remember {
        client.createBrowser("about:blank", CefRendering.CefRenderingWithHandler(panel, panel), false).also { created ->
            panel.browser = created
            created.createImmediately()
        }
    }

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

    SwingPanel(factory = { panel }, modifier = modifier)
}
