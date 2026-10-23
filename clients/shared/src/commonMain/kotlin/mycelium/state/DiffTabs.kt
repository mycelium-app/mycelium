package mycelium.state

data class DiffTab(val key: String, val title: String, val closeLabel: String, val hash: String? = null, val reveal: String? = null)

data class DiffTabs(val tabs: List<DiffTab> = emptyList(), val active: String? = null)

private const val ShortHashLength = 7

const val ChangesTabKey = "changes"

fun changesTab(reveal: String? = null): DiffTab =
    DiffTab(key = ChangesTabKey, title = "Changes", closeLabel = "Close Changes", reveal = reveal)

fun commitDiffTab(hash: String): DiffTab {
    val short = hash.take(ShortHashLength)

    return DiffTab(key = "commit:$hash", title = "Commit $short", closeLabel = "Close Commit $short", hash = hash)
}

fun DiffTabs.opened(tab: DiffTab): DiffTabs =
    DiffTabs(if (tabs.any { it.key == tab.key }) tabs.map { if (it.key == tab.key) tab else it } else tabs + tab, tab.key)

fun DiffTabs.revealed(key: String): DiffTabs =
    copy(tabs = tabs.map { if (it.key == key) it.copy(reveal = null) else it })

fun DiffTabs.closed(key: String): DiffTabs =
    DiffTabs(tabs.filter { it.key != key }, active.takeIf { it != key })
