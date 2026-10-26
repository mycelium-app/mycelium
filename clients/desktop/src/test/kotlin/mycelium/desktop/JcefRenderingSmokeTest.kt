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
import org.cef.handler.CefContextMenuHandler
import org.cef.handler.CefDisplayHandlerAdapter
import org.cef.CefSettings
import java.util.concurrent.LinkedBlockingQueue
import java.awt.event.KeyEvent
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.awt.Cursor
import java.awt.Rectangle
import java.awt.event.MouseEvent
import java.nio.ByteBuffer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import javax.swing.SwingUtilities
import javax.swing.JFrame
import javax.swing.JMenuItem
import javax.swing.JPopupMenu
import javax.swing.MenuSelectionManager
import java.awt.datatransfer.DataFlavor
import java.awt.Toolkit
import java.awt.event.InputEvent
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

        val dismissed = LinkedBlockingQueue<Unit>()
        val inputReady = CountDownLatch(1)
        val edited = CountDownLatch(1)
        val customMenu = CountDownLatch(1)
        val customReady = CountDownLatch(1)

        client.addDisplayHandler(object : CefDisplayHandlerAdapter() {
            override fun onConsoleMessage(browser: CefBrowser, level: CefSettings.LogSeverity, message: String, source: String, line: Int): Boolean {
                when (message) {
                    "input-ready" -> inputReady.countDown()
                    "input-cut" -> edited.countDown()
                    "custom-menu" -> customMenu.countDown()
                    "custom-ready" -> customReady.countDown()
                }

                return true
            }
        })
        client.addContextMenuHandler(object : CefContextMenuHandler by host.contextMenu {
            override fun onContextMenuDismissed(browser: CefBrowser, frame: CefFrame) {
                host.contextMenu.onContextMenuDismissed(browser, frame)
                SwingUtilities.invokeLater { dismissed.offer(Unit) }
            }
        })
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
        val owner = JFrame("Synthetic browser test")
        val menuShown = CountDownLatch(1)
        val menuSelection = MenuSelectionManager.defaultManager()
        val menuListener = javax.swing.event.ChangeListener {
            if (menuSelection.selectedPath.any { it is JPopupMenu }) {
                menuShown.countDown()
            }
        }

        SwingUtilities.invokeAndWait {
            owner.setSize(800, 600)
            owner.isVisible = true
            host.place(owner.locationOnScreen, owner)
            menuSelection.addChangeListener(menuListener)
        }
        host.resize(800, 600, 2.0)
        val browser = client.createBrowser("about:blank", CefRendering.CefRenderingWithHandler(rendering, host.source), false)

        try {
            host.browser = browser
            browser.createImmediately()
            assertTrue(created.await(20, TimeUnit.SECONDS), "Native browser was not created")
            browser.loadURL("data:text/html,<html><body style='margin:0;background:rgb(18,52,86)'><div style='width:100px;height:100px;background:white;cursor:pointer'><a href='https://example.invalid/link'>Link</a></div></body></html>")
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
            host.mouse(MouseEvent(host.source, MouseEvent.MOUSE_PRESSED, 0, InputEvent.BUTTON3_DOWN_MASK, 20, 20, 1, true, MouseEvent.BUTTON3), Offset(20f, 20f))
            host.mouse(MouseEvent(host.source, MouseEvent.MOUSE_RELEASED, 0, 0, 20, 20, 1, true, MouseEvent.BUTTON3), Offset(20f, 20f))
            assertTrue(menuShown.await(5, TimeUnit.SECONDS), "Right-click did not display the native page menu in the app")
            SwingUtilities.invokeAndWait {
                val menu = menuSelection.selectedPath.filterIsInstance<JPopupMenu>().first()
                val copy = menu.components.filterIsInstance<JMenuItem>().single { it.text == "Copy link address" }

                copy.doClick()
                assertTrue(!menu.isVisible)
            }
            assertNotNull(dismissed.poll(5, TimeUnit.SECONDS), "Selected menu command did not dismiss its native menu")
            SwingUtilities.invokeAndWait {
                assertEquals("https://example.invalid/link", Toolkit.getDefaultToolkit().systemClipboard.getData(DataFlavor.stringFlavor))
            }
            browser.executeJavaScript("const input = document.createElement('input'); input.value = 'Editable text'; document.body.append(input); input.focus(); input.select(); input.addEventListener('input', () => console.log('input-cut')); console.log('input-ready');", browser.url, 0)
            assertTrue(inputReady.await(5, TimeUnit.SECONDS))
            browser.setFocus(true)
            val keyboardMenu = CountDownLatch(1)
            val keyboardListener = javax.swing.event.ChangeListener {
                if (menuSelection.selectedPath.any { it is JPopupMenu }) {
                    keyboardMenu.countDown()
                }
            }

            SwingUtilities.invokeAndWait { menuSelection.addChangeListener(keyboardListener) }
            host.key(KeyEvent(host.source, KeyEvent.KEY_PRESSED, 0, InputEvent.SHIFT_DOWN_MASK, KeyEvent.VK_F10, KeyEvent.CHAR_UNDEFINED))
            host.key(KeyEvent(host.source, KeyEvent.KEY_RELEASED, 0, InputEvent.SHIFT_DOWN_MASK, KeyEvent.VK_F10, KeyEvent.CHAR_UNDEFINED))
            assertTrue(keyboardMenu.await(5, TimeUnit.SECONDS), "Shift+F10 did not open the edit menu")
            SwingUtilities.invokeAndWait {
                menuSelection.removeChangeListener(keyboardListener)
                val menu = menuSelection.selectedPath.filterIsInstance<JPopupMenu>().first()
                val cut = menu.components.filterIsInstance<JMenuItem>().single { it.text == "Cut" }

                assertTrue(cut.isEnabled)
                cut.doClick()
            }
            assertTrue(edited.await(5, TimeUnit.SECONDS), "The native Cut command did not edit the focused input")
            assertNotNull(dismissed.poll(5, TimeUnit.SECONDS))
            val reopened = CountDownLatch(1)
            val reopenListener = javax.swing.event.ChangeListener {
                if (menuSelection.selectedPath.any { it is JPopupMenu }) {
                    reopened.countDown()
                }
            }

            SwingUtilities.invokeAndWait { menuSelection.addChangeListener(reopenListener) }
            host.key(KeyEvent(host.source, KeyEvent.KEY_PRESSED, 0, InputEvent.SHIFT_DOWN_MASK, KeyEvent.VK_F10, KeyEvent.CHAR_UNDEFINED))
            host.key(KeyEvent(host.source, KeyEvent.KEY_RELEASED, 0, InputEvent.SHIFT_DOWN_MASK, KeyEvent.VK_F10, KeyEvent.CHAR_UNDEFINED))
            assertTrue(reopened.await(5, TimeUnit.SECONDS))
            SwingUtilities.invokeAndWait {
                menuSelection.removeChangeListener(reopenListener)
                menuSelection.clearSelectedPath()
            }
            assertNotNull(dismissed.poll(5, TimeUnit.SECONDS), "Dismissal did not cancel the native menu callback")
            browser.executeJavaScript("document.addEventListener('contextmenu', event => { event.preventDefault(); console.log('custom-menu'); }); console.log('custom-ready');", browser.url, 0)
            assertTrue(customReady.await(5, TimeUnit.SECONDS))
            host.mouse(MouseEvent(host.source, MouseEvent.MOUSE_PRESSED, 0, InputEvent.BUTTON3_DOWN_MASK, 250, 50, 1, true, MouseEvent.BUTTON3), Offset(250f, 50f))
            host.mouse(MouseEvent(host.source, MouseEvent.MOUSE_RELEASED, 0, 0, 250, 50, 1, true, MouseEvent.BUTTON3), Offset(250f, 50f))
            assertTrue(customMenu.await(5, TimeUnit.SECONDS), "The page did not receive its custom context-menu event")
            SwingUtilities.invokeAndWait { assertTrue(menuSelection.selectedPath.none { it is JPopupMenu }) }
        } finally {
            host.contextMenu.close()
            browser.close(true)
            client.dispose()
            app.dispose()
            SwingUtilities.invokeAndWait {
                menuSelection.removeChangeListener(menuListener)
                owner.dispose()
            }
        }
    }
}
