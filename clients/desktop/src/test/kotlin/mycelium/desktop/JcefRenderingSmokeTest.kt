package mycelium.desktop

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.input.pointer.PointerIcon
import com.jetbrains.cef.JCefAppConfig
import org.cef.CefApp
import org.cef.browser.CefBrowser
import org.cef.browser.CefFrame
import org.cef.browser.CefRendering
import org.cef.handler.CefLoadHandlerAdapter
import org.cef.handler.CefLifeSpanHandlerAdapter
import org.cef.handler.CefRenderHandler
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.awt.Cursor
import java.awt.Rectangle
import java.awt.event.MouseEvent
import java.nio.ByteBuffer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import javax.swing.SwingUtilities
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class JcefRenderingSmokeTest {
    @Test
    fun aNativeBrowserPaintsAtPhysicalResolutionWithScaledContentAndThePagesPointer() {
        assumeTrue(System.getenv("MYCELIUM_JCEF_SMOKE") == "1")
        val config = JCefAppConfig.getInstance()
        val args = config.appArgs + arrayOf("--no-sandbox", "--disable-gpu")

        assertTrue(CefApp.startup(args))
        val app = CefApp.getInstance(args, config.cefSettings)
        val initialised = CountDownLatch(1)

        app.onInitialization { initialised.countDown() }
        assertTrue(initialised.await(20, TimeUnit.SECONDS), "CEF did not initialise")
        val client = app.createClient()
        val host = OffscreenBrowser()
        val created = CountDownLatch(1)
        val loaded = CountDownLatch(1)
        val painted = CountDownLatch(1)
        val pointed = CountDownLatch(1)
        val rendering = object : CefRenderHandler by host {
            override fun onPaint(browser: CefBrowser, popup: Boolean, dirtyRects: Array<out Rectangle>, buffer: ByteBuffer, width: Int, height: Int) {
                host.onPaint(browser, popup, dirtyRects, buffer, width, height)

                if (!popup && width == 800 && height == 600 && buffer.get((50 * width + 150) * 4).toInt() == -1 && buffer.get((50 * width + 250) * 4).toInt() == 86) {
                    painted.countDown()
                }
            }

            override fun onCursorChange(browser: CefBrowser, cursorType: Int): Boolean {
                val handled = host.onCursorChange(browser, cursorType)

                if (cursorType == Cursor.HAND_CURSOR) {
                    pointed.countDown()
                }

                return handled
            }
        }

        client.addLifeSpanHandler(object : CefLifeSpanHandlerAdapter() {
            override fun onAfterCreated(browser: CefBrowser) {
                created.countDown()
            }
        })
        client.addLoadHandler(object : CefLoadHandlerAdapter() {
            override fun onLoadEnd(browser: CefBrowser, frame: CefFrame, httpStatusCode: Int) {
                host.loaded.onLoadEnd(browser, frame, httpStatusCode)

                if (frame.isMain && browser.url.startsWith("data:")) {
                    loaded.countDown()
                }
            }
        })
        host.resize(800, 600, 2.0)
        val browser = client.createBrowser("about:blank", CefRendering.CefRenderingWithHandler(rendering, host.source), false)

        try {
            host.browser = browser
            browser.createImmediately()
            assertTrue(created.await(20, TimeUnit.SECONDS), "Native browser was not created")
            browser.loadURL("data:text/html,<html><body style='margin:0;background:rgb(18,52,86)'><div style='width:100px;height:100px;background:white;cursor:pointer'></div></body></html>")
            assertTrue(loaded.await(20, TimeUnit.SECONDS), "Synthetic page did not load")
            assertTrue(painted.await(20, TimeUnit.SECONDS), "CEF did not paint a physical-size frame at 2x page zoom")
            val image = assertNotNull(host.frame())
            val pixels = image.toPixelMap()

            assertEquals(800 to 600, image.width to image.height)
            assertEquals(Color.White, pixels[150, 50])
            assertEquals(Color(18, 52, 86), pixels[250, 50])
            assertEquals(pageZoomLevel(2.0), browser.zoomLevel, 1e-6)
            host.mouse(MouseEvent(host.source, MouseEvent.MOUSE_MOVED, 0, 0, 50, 50, 0, false), Offset(50f, 50f))
            assertTrue(pointed.await(10, TimeUnit.SECONDS), "CEF did not forward the page cursor")
            SwingUtilities.invokeAndWait {}
            assertEquals(PointerIcon(Cursor.getPredefinedCursor(Cursor.HAND_CURSOR)), host.cursor)
        } finally {
            browser.close(true)
            client.dispose()
            app.dispose()
        }
    }
}
