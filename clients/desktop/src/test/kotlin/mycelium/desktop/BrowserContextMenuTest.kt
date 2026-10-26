package mycelium.desktop

import org.cef.browser.CefBrowser
import org.cef.browser.CefFrame
import org.cef.callback.CefContextMenuParams
import org.cef.callback.CefMenuModel
import org.cef.callback.CefRunContextMenuCallback
import java.lang.reflect.Proxy
import javax.swing.JCheckBoxMenuItem
import javax.swing.JMenu
import javax.swing.JMenuItem
import javax.swing.SwingUtilities
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

class BrowserContextMenuTest {
    @Test
    fun nativeMenuItemsKeepTheirLabelsStateAndNestedCommands() {
        val selected = mutableListOf<Int>()
        val entries = listOf(
            BrowserMenuItem(1, "Back", enabled = false),
            BrowserMenuItem(2, "Checked", type = CefMenuModel.MenuItemType.MENUITEMTYPE_CHECK, checked = true),
            BrowserMenuItem(3, "More", type = CefMenuModel.MenuItemType.MENUITEMTYPE_SUBMENU, children = listOf(BrowserMenuItem(4, "Copy"))),
        )

        SwingUtilities.invokeAndWait {
            val menu = browserPopupMenu(entries) { selected.add(it) }
            val back = assertIs<JMenuItem>(menu.getComponent(0))
            val checked = assertIs<JCheckBoxMenuItem>(menu.getComponent(1))
            val more = assertIs<JMenu>(menu.getComponent(2))

            assertEquals("Back", back.text)
            assertFalse(back.isEnabled)
            assertTrue(checked.isSelected)
            back.doClick()
            more.getItem(0).doClick()
        }

        assertEquals(listOf(4), selected)
    }

    @Test
    fun aMenuWithoutAShowingWindowCancelsOnceAndClosingDoesNotCompleteItAgain() {
        val handler = BrowserContextMenu()
        var cancelled = 0
        var continued = 0
        val callback = object : CefRunContextMenuCallback {
            override fun cancel() {
                cancelled++
            }

            override fun Continue(commandId: Int, eventFlags: Int) {
                continued++
            }
        }
        val model = Proxy.newProxyInstance(CefMenuModel::class.java.classLoader, arrayOf(CefMenuModel::class.java)) { _, _, _ -> 0 } as CefMenuModel
        val params = Proxy.newProxyInstance(CefContextMenuParams::class.java.classLoader, arrayOf(CefContextMenuParams::class.java)) { _, _, _ -> 0 } as CefContextMenuParams
        val browser = Proxy.newProxyInstance(CefBrowser::class.java.classLoader, arrayOf(CefBrowser::class.java)) { _, _, _ -> null } as CefBrowser
        val frame = Proxy.newProxyInstance(CefFrame::class.java.classLoader, arrayOf(CefFrame::class.java)) { _, _, _ -> null } as CefFrame

        assertTrue(handler.runContextMenu(browser, frame, params, model, callback))
        SwingUtilities.invokeAndWait {}
        handler.close()
        SwingUtilities.invokeAndWait {}

        assertEquals(1, cancelled)
        assertEquals(0, continued)
    }
}
