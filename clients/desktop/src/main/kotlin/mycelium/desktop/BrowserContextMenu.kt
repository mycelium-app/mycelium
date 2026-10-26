package mycelium.desktop

import org.cef.browser.CefBrowser
import org.cef.browser.CefFrame
import org.cef.callback.CefContextMenuParams
import org.cef.callback.CefMenuModel
import org.cef.callback.CefRunContextMenuCallback
import org.cef.handler.CefContextMenuHandlerAdapter
import org.cef.misc.BoolRef
import org.cef.misc.IntRef
import java.awt.Component
import java.awt.Point
import java.awt.Toolkit
import java.awt.datatransfer.StringSelection
import java.awt.event.InputEvent
import javax.swing.ButtonGroup
import javax.swing.JCheckBoxMenuItem
import javax.swing.JMenu
import javax.swing.JMenuItem
import javax.swing.JPopupMenu
import javax.swing.JRadioButtonMenuItem
import javax.swing.KeyStroke
import javax.swing.MenuSelectionManager
import javax.swing.SwingUtilities
import javax.swing.event.PopupMenuEvent
import javax.swing.event.PopupMenuListener

private const val OpenLink = CefMenuModel.MenuId.MENU_ID_USER_FIRST
private const val CopyLink = OpenLink + 1

internal data class BrowserMenuItem(
    val command: Int,
    val label: String,
    val type: CefMenuModel.MenuItemType = CefMenuModel.MenuItemType.MENUITEMTYPE_COMMAND,
    val enabled: Boolean = true,
    val checked: Boolean = false,
    val group: Int = 0,
    val accelerator: KeyStroke? = null,
    val children: List<BrowserMenuItem> = emptyList(),
)

internal fun browserMenuItems(model: CefMenuModel): List<BrowserMenuItem> = (0 until model.count).mapNotNull { index ->
    if (!model.isVisibleAt(index)) {
        return@mapNotNull null
    }

    val key = IntRef()
    val shift = BoolRef()
    val control = BoolRef()
    val alt = BoolRef()
    val accelerated = model.getAcceleratorAt(index, key, shift, control, alt)
    val modifiers = (if (shift.get()) InputEvent.SHIFT_DOWN_MASK else 0) or
        (if (control.get()) InputEvent.CTRL_DOWN_MASK else 0) or
        (if (alt.get()) InputEvent.ALT_DOWN_MASK else 0)
    val children = model.getSubMenuAt(index)?.let(::browserMenuItems).orEmpty()

    BrowserMenuItem(model.getCommandIdAt(index), model.getLabelAt(index).orEmpty().replace("&&", "\u0000").replace("&", "").replace("\u0000", "&"), model.getTypeAt(index), model.isEnabledAt(index), model.isCheckedAt(index), model.getGroupIdAt(index), if (accelerated) KeyStroke.getKeyStroke(key.get(), modifiers) else null, children)
}

internal fun browserPopupMenu(entries: List<BrowserMenuItem>, select: (Int) -> Unit): JPopupMenu {
    val menu = JPopupMenu()
    val groups = mutableMapOf<Int, ButtonGroup>()

    menu.isLightWeightPopupEnabled = false

    for (entry in entries) {
        if (entry.type == CefMenuModel.MenuItemType.MENUITEMTYPE_SEPARATOR) {
            menu.addSeparator()
            continue
        }

        val item = when (entry.type) {
            CefMenuModel.MenuItemType.MENUITEMTYPE_CHECK -> JCheckBoxMenuItem(entry.label, entry.checked)
            CefMenuModel.MenuItemType.MENUITEMTYPE_RADIO -> JRadioButtonMenuItem(entry.label, entry.checked).also { groups.getOrPut(entry.group) { ButtonGroup() }.add(it) }
            CefMenuModel.MenuItemType.MENUITEMTYPE_SUBMENU -> JMenu(entry.label).also { submenu ->
                val children = browserPopupMenu(entry.children, select)

                children.components.forEach { submenu.add(it) }
            }
            else -> JMenuItem(entry.label)
        }

        item.isEnabled = entry.enabled

        if (item !is JMenu) {
            item.accelerator = entry.accelerator
            item.addActionListener { select(entry.command) }
        }

        menu.add(item)
    }

    return menu
}

class BrowserContextMenu : CefContextMenuHandlerAdapter(), AutoCloseable {
    var owner: Component? = null
    var origin = Point()
    var screenScale = 1.0
    private var popup: JPopupMenu? = null
    private var pending: CefRunContextMenuCallback? = null
    private var closed = false

    override fun onBeforeContextMenu(browser: CefBrowser, frame: CefFrame, params: CefContextMenuParams, model: CefMenuModel) {
        if (params.linkUrl.isNotBlank()) {
            if (model.count > 0) {
                model.addSeparator()
            }

            model.addItem(OpenLink, "Open link")
            model.addItem(CopyLink, "Copy link address")
        }
    }

    override fun runContextMenu(browser: CefBrowser, frame: CefFrame, params: CefContextMenuParams, model: CefMenuModel, callback: CefRunContextMenuCallback): Boolean {
        val entries = browserMenuItems(model)
        val position = Point(origin.x + (params.xCoord / screenScale).toInt(), origin.y + (params.yCoord / screenScale).toInt())

        SwingUtilities.invokeLater {
            finish(null)
            pending = callback
            val anchor = owner

            if (closed || anchor?.isShowing != true || entries.isEmpty()) {
                finish(null)
                return@invokeLater
            }

            val menu = browserPopupMenu(entries) { finish(it) }

            popup = menu
            menu.addPopupMenuListener(object : PopupMenuListener {
                override fun popupMenuWillBecomeVisible(event: PopupMenuEvent) = Unit

                override fun popupMenuWillBecomeInvisible(event: PopupMenuEvent) {
                    SwingUtilities.invokeLater {
                        if (popup === menu) {
                            finish(null)
                        }
                    }
                }

                override fun popupMenuCanceled(event: PopupMenuEvent) = popupMenuWillBecomeInvisible(event)
            })
            SwingUtilities.convertPointFromScreen(position, anchor)
            menu.show(anchor, position.x, position.y)
        }

        return true
    }

    override fun onContextMenuCommand(browser: CefBrowser, frame: CefFrame, params: CefContextMenuParams, commandId: Int, eventFlags: Int): Boolean = when (commandId) {
        OpenLink -> {
            browser.loadURL(params.linkUrl)
            true
        }
        CopyLink -> {
            val link = params.linkUrl

            SwingUtilities.invokeLater { Toolkit.getDefaultToolkit().systemClipboard.setContents(StringSelection(link), null) }
            true
        }
        else -> false
    }

    override fun onContextMenuDismissed(browser: CefBrowser, frame: CefFrame) {
        SwingUtilities.invokeLater { finish(null) }
    }

    private fun finish(command: Int?) {
        val callback = pending
        val shown = popup

        pending = null
        popup = null
        shown?.isVisible = false

        if (shown != null && MenuSelectionManager.defaultManager().selectedPath.any { it === shown }) {
            MenuSelectionManager.defaultManager().clearSelectedPath()
        }

        if (command == null) {
            callback?.cancel()
        } else {
            callback?.Continue(command, 0)
        }
    }

    override fun close() {
        val closing = Runnable {
            closed = true
            finish(null)
        }

        if (SwingUtilities.isEventDispatchThread()) {
            closing.run()
        } else {
            SwingUtilities.invokeAndWait(closing)
        }
    }
}
