package mycelium.state

data class DiffTab(val key: String, val title: String, val closeLabel: String, val path: String? = null, val staged: Boolean = false, val hash: String? = null)

data class DiffTabs(val tabs: List<DiffTab> = emptyList(), val active: String? = null)

private const val ShortHashLength = 7

fun fileDiffTab(path: String, staged: Boolean): DiffTab =
    DiffTab(key = "file:$path", title = path.substringAfterLast('/'), closeLabel = "Close diff of $path", path = path, staged = staged)

fun commitDiffTab(hash: String, subject: String): DiffTab {
    val short = hash.take(ShortHashLength)

    return DiffTab(key = "commit:$hash", title = "$short $subject", closeLabel = "Close diff of $short $subject", hash = hash)
}

fun DiffTabs.opened(tab: DiffTab): DiffTabs =
    DiffTabs(if (tabs.any { it.key == tab.key }) tabs.map { if (it.key == tab.key) tab else it } else tabs + tab, tab.key)

fun DiffTabs.closed(key: String): DiffTabs =
    DiffTabs(tabs.filter { it.key != key }, active.takeIf { it != key })
