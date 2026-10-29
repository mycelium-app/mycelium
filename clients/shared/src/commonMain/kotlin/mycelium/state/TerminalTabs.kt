package mycelium.state

enum class SplitDirection { Right, Down }

sealed interface TerminalLayout {
    data class Pane(val id: String) : TerminalLayout

    data class Split(val direction: SplitDirection, val first: TerminalLayout, val second: TerminalLayout) : TerminalLayout
}

data class TerminalTab(val id: String, val number: Int, val layout: TerminalLayout, val focused: String) {
    val title: String get() = "Terminal $number"
}

data class TerminalTabs(val tabs: List<TerminalTab> = emptyList(), val selected: String? = null, private val created: Int = 0) {
    val current: TerminalTab? get() = tabs.firstOrNull { it.id == selected }

    fun open(): TerminalTabs {
        val number = created + 1
        val pane = "pane$number"
        val tab = TerminalTab("tab$number", number, TerminalLayout.Pane(pane), pane)

        return copy(tabs = tabs + tab, selected = tab.id, created = number)
    }

    fun select(tabId: String): TerminalTabs = if (tabs.any { it.id == tabId }) copy(selected = tabId) else this

    fun focus(paneId: String): TerminalTabs = copy(tabs = tabs.map { if (paneId in it.layout.paneIds()) it.copy(focused = paneId) else it })

    fun split(direction: SplitDirection): TerminalTabs {
        val tab = current ?: return open()
        val number = created + 1
        val pane = "pane$number"
        val split = tab.copy(layout = tab.layout.splitting(tab.focused, direction, pane), focused = pane)

        return copy(tabs = tabs.map { if (it.id == tab.id) split else it }, created = number)
    }

    fun closePane(paneId: String): TerminalTabs {
        val tab = tabs.firstOrNull { paneId in it.layout.paneIds() } ?: return this
        val remaining = tab.layout.without(paneId)

        if (remaining == null) {
            val kept = tabs.filter { it.id != tab.id }
            val next = if (selected == tab.id) kept.getOrNull((tabs.indexOf(tab) - 1).coerceAtLeast(0))?.id else selected

            return copy(tabs = kept, selected = next)
        }

        val focused = if (tab.focused == paneId) remaining.paneIds().first() else tab.focused

        return copy(tabs = tabs.map { if (it.id == tab.id) it.copy(layout = remaining, focused = focused) else it })
    }

    fun closeFocused(): TerminalTabs = current?.let { closePane(it.focused) } ?: this

    fun closeTab(tabId: String): TerminalTabs = tabs.firstOrNull { it.id == tabId }?.layout?.paneIds()?.fold(this) { result, pane -> result.closePane(pane) } ?: this
}

fun TerminalLayout.paneIds(): List<String> = when (this) {
    is TerminalLayout.Pane -> listOf(id)
    is TerminalLayout.Split -> first.paneIds() + second.paneIds()
}

private fun TerminalLayout.splitting(paneId: String, direction: SplitDirection, added: String): TerminalLayout = when (this) {
    is TerminalLayout.Pane -> if (id == paneId) TerminalLayout.Split(direction, this, TerminalLayout.Pane(added)) else this
    is TerminalLayout.Split -> copy(first = first.splitting(paneId, direction, added), second = second.splitting(paneId, direction, added))
}

private fun TerminalLayout.without(paneId: String): TerminalLayout? = when (this) {
    is TerminalLayout.Pane -> if (id == paneId) null else this
    is TerminalLayout.Split -> {
        val left = first.without(paneId)
        val right = second.without(paneId)

        when {
            left == null -> right
            right == null -> left
            else -> copy(first = left, second = right)
        }
    }
}

data class PaneRect(val x: Float, val y: Float, val width: Float, val height: Float)

fun TerminalLayout.rects(within: PaneRect = PaneRect(0f, 0f, 1f, 1f)): Map<String, PaneRect> = when (this) {
    is TerminalLayout.Pane -> mapOf(id to within)
    is TerminalLayout.Split -> when (direction) {
        SplitDirection.Right -> first.rects(within.copy(width = within.width / 2)) + second.rects(within.copy(x = within.x + within.width / 2, width = within.width / 2))
        SplitDirection.Down -> first.rects(within.copy(height = within.height / 2)) + second.rects(within.copy(y = within.y + within.height / 2, height = within.height / 2))
    }
}
