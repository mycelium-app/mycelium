package mycelium.desktop

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.SideEffect
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
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import com.jetbrains.cef.JCefAppConfig
import mycelium.PageReading
import mycelium.ui.LocalRetainedViewKey
import mycelium.ui.RetainedViews
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
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

private const val AttachedFrameRate = 60

private const val DetachedFrameRate = 1

private const val RetainedSessionLimit = 4

private val retainedSessions = RetainedViews<JcefSession>(RetainedSessionLimit) { it.close() }

private class JcefSession {
    val host = OffscreenBrowser()
    var reading: PageReading? = null
    var onDisconnected: (() -> Unit)? = null
    val reader = PageReader { reading }
    val statuses = LiveViewStatus { onDisconnected }
    var browserCreated by mutableStateOf(false)
    var browser by mutableStateOf<CefBrowser?>(null)
    var loadedUrl: String? = null
    val client: CefClient = Jcef.createClient().also {
        it.addLoadHandler(host.loaded)
        it.addContextMenuHandler(host.contextMenu)
        it.addMessageRouter(reader.router)
        it.addMessageRouter(statuses.router)
        it.addLifeSpanHandler(object : CefLifeSpanHandlerAdapter() {
            override fun onAfterCreated(browser: CefBrowser) {
                SwingUtilities.invokeLater { browserCreated = true }
            }
        })
    }

    fun close() {
        reader.router.dispose()
        statuses.router.dispose()
        browser?.close(true)
        client.dispose()
    }
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun JcefWebContent(url: String, token: String?, modifier: Modifier, reading: PageReading? = null, viewOnly: Boolean? = null, onPointer: (() -> Unit)? = null, onDisconnected: (() -> Unit)? = null) {
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

    val retainKey = LocalRetainedViewKey.current
    val session = remember(retainKey) { if (retainKey != null) retainedSessions.obtain(retainKey) { JcefSession() } else JcefSession() }
    val host = session.host
    val currentOnPointer by rememberUpdatedState(onPointer)
    val browser = session.browser
    val browserCreated = session.browserCreated
    var laidOut by remember { mutableStateOf(false) }
    val density = LocalDensity.current.density.toDouble()
    val focus = remember { FocusRequester() }
    val bounds = rememberBrowserBounds(host, density) { laidOut = it }

    SideEffect {
        session.reading = reading
        session.onDisconnected = onDisconnected
    }

    DisposableEffect(session) {
        session.browser?.setWindowlessFrameRate(AttachedFrameRate)

        onDispose {
            host.contextMenu.close()
            session.reading = null
            session.onDisconnected = null

            if (retainKey == null) {
                session.close()
            } else {
                session.browser?.setWindowlessFrameRate(DetachedFrameRate)
            }
        }
    }

    LaunchedEffect(laidOut) {
        if (laidOut && session.browser == null) {
            session.browser = session.client.createBrowser("about:blank", CefRendering.CefRenderingWithHandler(host, host.source), false).also { created ->
                host.browser = created
                created.createImmediately()
                created.setWindowlessFrameRate(AttachedFrameRate)
                host.applyZoom()
            }
        }
    }

    LaunchedEffect(url, token, browser, browserCreated) {
        val created = browser ?: return@LaunchedEffect

        if (!browserCreated || session.loadedUrl == url) {
            return@LaunchedEffect
        }

        if (token != null) {
            Jcef.setSessionCookie(url, token)
        }

        session.loadedUrl = url
        created.loadURL(url)
    }

    LaunchedEffect(browser, browserCreated, reading != null) {
        val created = browser ?: return@LaunchedEffect

        while (reading != null && browserCreated) {
            delay(ReadIntervalMillis)
            session.reader.read(created)
        }
    }

    LaunchedEffect(browser, browserCreated, viewOnly) {
        val created = browser ?: return@LaunchedEffect

        if (viewOnly == null || !browserCreated) {
            return@LaunchedEffect
        }

        for (wait in ControlResendMillis) {
            delay(wait)
            created.executeJavaScript("window.postMessage({type: 'control', viewOnly: $viewOnly}, '*')", created.url, 0)
        }
    }

    Canvas(
        modifier
            .then(bounds)
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
                            currentOnPointer?.invoke()
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

@Composable
internal fun rememberBrowserBounds(host: OffscreenBrowser, density: Double, onLaidOut: (Boolean) -> Unit): Modifier {
    var coordinates by remember { mutableStateOf<LayoutCoordinates?>(null) }
    val currentOnLaidOut by rememberUpdatedState(onLaidOut)

    LaunchedEffect(host, density) {
        while (true) {
            val attached = coordinates?.takeIf { it.isAttached }

            if (attached != null) {
                host.follow(attached, density)
                currentOnLaidOut(attached.size.width > 0 && attached.size.height > 0)
            }

            delay(BoundsPollMillis)
        }
    }

    return Modifier.onGloballyPositioned { coordinates = it }
}

private fun OffscreenBrowser.follow(coordinates: LayoutCoordinates, density: Double) {
    val inWindow = coordinates.positionInWindow()
    val window = java.awt.KeyboardFocusManager.getCurrentKeyboardFocusManager().activeWindow
    val windowOrigin = if (window?.isShowing == true) window.locationOnScreen else java.awt.Point(0, 0)
    val screenScale = window?.graphicsConfiguration?.defaultTransform?.scaleX ?: 1.0

    resize(coordinates.size.width, coordinates.size.height, density)
    place(java.awt.Point(windowOrigin.x + (inWindow.x / screenScale).toInt(), windowOrigin.y + (inWindow.y / screenScale).toInt()), window, screenScale)
}

private const val ReadIntervalMillis = 3000L
private const val BoundsPollMillis = 50L
private val ControlResendMillis = listOf(300L, 700L, 1500L, 3000L)

private class LiveViewStatus(private val onDisconnected: () -> (() -> Unit)?) {
    val router: CefMessageRouter = CefMessageRouter.create().also { router ->
        router.addHandler(
            object : CefMessageRouterHandlerAdapter() {
                override fun onQuery(browser: CefBrowser, frame: org.cef.browser.CefFrame, queryId: Long, request: String, persistent: Boolean, callback: CefQueryCallback): Boolean {
                    val disconnected = onDisconnected() ?: return false

                    if (!isDisconnectedStatus(request)) {
                        return false
                    }

                    callback.success("")
                    SwingUtilities.invokeLater { disconnected() }

                    return true
                }
            },
            false,
        )
    }
}

internal fun isDisconnectedStatus(request: String): Boolean =
    runCatching { Json.parseToJsonElement(request).jsonObject }.getOrNull()?.let { it["type"]?.jsonPrimitive?.content == "status" && it["state"]?.jsonPrimitive?.content == "disconnected" } == true

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
