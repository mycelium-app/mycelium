package mycelium.desktop

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.awt.awtEventOrNull
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.pointerHoverIcon
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import com.jetbrains.cef.JCefAppConfig
import mycelium.PageReading
import org.cef.browser.CefMessageRouter
import org.cef.callback.CefQueryCallback
import org.cef.handler.CefMessageRouterHandlerAdapter
import org.cef.CefApp
import org.cef.CefSettings
import org.cef.CefClient
import org.cef.browser.CefBrowser
import org.cef.browser.CefRendering
import org.cef.handler.CefLifeSpanHandlerAdapter
import javax.swing.SwingUtilities
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

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun JcefWebContent(url: String, token: String?, modifier: Modifier, reading: PageReading? = null) {
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

    val host = remember { OffscreenBrowser() }
    val currentReading by rememberUpdatedState(reading)
    val reader = remember { PageReader { currentReading } }
    var browserCreated by remember { mutableStateOf(false) }
    val client = remember {
        Jcef.createClient().also {
            it.addLoadHandler(host.loaded)
            it.addContextMenuHandler(host.contextMenu)
            it.addMessageRouter(reader.router)
            it.addLifeSpanHandler(object : CefLifeSpanHandlerAdapter() {
                override fun onAfterCreated(browser: CefBrowser) {
                    SwingUtilities.invokeLater { browserCreated = true }
                }
            })
        }
    }
    var browser by remember { mutableStateOf<CefBrowser?>(null) }
    var laidOut by remember { mutableStateOf(false) }
    val density = LocalDensity.current.density.toDouble()
    val focus = remember { FocusRequester() }

    DisposableEffect(Unit) {
        onDispose {
            host.contextMenu.close()
            reader.router.dispose()
            browser?.close(true)
            client.dispose()
        }
    }

    LaunchedEffect(laidOut) {
        if (laidOut && browser == null) {
            browser = client.createBrowser("about:blank", CefRendering.CefRenderingWithHandler(host, host.source), false).also { created ->
                host.browser = created
                created.createImmediately()
                created.setWindowlessFrameRate(60)
                host.applyZoom()
            }
        }
    }

    LaunchedEffect(url, token, browser, browserCreated) {
        val created = browser ?: return@LaunchedEffect

        if (!browserCreated) {
            return@LaunchedEffect
        }

        if (token != null) {
            Jcef.setSessionCookie(url, token)
        }

        created.loadURL(url)
    }

    LaunchedEffect(browser, browserCreated, reading != null) {
        val created = browser ?: return@LaunchedEffect

        while (reading != null && browserCreated) {
            delay(ReadIntervalMillis)
            reader.read(created)
        }
    }

    Canvas(
        modifier
            .onGloballyPositioned { coordinates ->
                val inWindow = coordinates.positionInWindow()
                val window = java.awt.KeyboardFocusManager.getCurrentKeyboardFocusManager().activeWindow
                val windowOrigin = if (window?.isShowing == true) window.locationOnScreen else java.awt.Point(0, 0)
                val screenScale = window?.graphicsConfiguration?.defaultTransform?.scaleX ?: 1.0

                host.resize(coordinates.size.width, coordinates.size.height, density)
                laidOut = coordinates.size.width > 0 && coordinates.size.height > 0
                host.place(java.awt.Point(windowOrigin.x + (inWindow.x / screenScale).toInt(), windowOrigin.y + (inWindow.y / screenScale).toInt()), window, screenScale)
            }
            .onKeyEvent { event ->
                val native = event.awtEventOrNull ?: return@onKeyEvent false

                host.key(native)
                true
            }
            .focusRequester(focus)
            .focusable()
            .pointerInput(host) {
                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent()
                        val native = event.nativeEvent as? java.awt.event.MouseEvent ?: continue
                        val change = event.changes.firstOrNull() ?: continue

                        if (event.type == PointerEventType.Press) {
                            focus.requestFocus()
                            browser?.setFocus(true)
                        }

                        host.mouse(native, change.position)
                        event.changes.forEach { it.consume() }
                    }
                }
            }
            .pointerHoverIcon(host.cursor)
            .clipToBounds(),
    ) {
        host.frames
        val frame = host.frame() ?: return@Canvas

        drawImage(frame)
    }
}

private const val ReadIntervalMillis = 3000L

private class PageReader(private val reading: () -> PageReading?) {
    val router: CefMessageRouter = CefMessageRouter.create().also { router ->
        router.addHandler(
            object : CefMessageRouterHandlerAdapter() {
                override fun onQuery(browser: CefBrowser, frame: org.cef.browser.CefFrame, queryId: Long, request: String, persistent: Boolean, callback: CefQueryCallback): Boolean {
                    val current = reading() ?: return false

                    if (!current.allowedUrl(browser.url)) {
                        return false
                    }

                    current.onRead(request)
                    callback.success("")

                    return true
                }
            },
            true,
        )
    }

    fun read(browser: CefBrowser) {
        val current = reading() ?: return

        if (!current.allowedUrl(browser.url)) {
            return
        }

        browser.executeJavaScript("window.cefQuery({request: ${current.script}, onSuccess: function(){}, onFailure: function(){}})", browser.url, 0)
    }
}
