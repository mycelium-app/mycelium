package mycelium.ui

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import mycelium.PageReading
import mycelium.Platform
import mycelium.api.ApiException
import mycelium.api.ChatToolServer
import mycelium.api.MyceliumApi
import mycelium.api.OutlineCollection
import mycelium.api.OutlineDocumentNode
import mycelium.state.OutlinePage
import mycelium.state.outlineAncestors
import mycelium.state.outlineDocumentIds
import mycelium.state.outlinePageOf

private const val OutlineSignInPath = "/"

private val TreeHeight = 300.dp

private val TreeCollapsesBelow = 40.dp

private val DocumentMinHeight = 160.dp

private val CollapsedBelowPanelHeight = 480.dp

private val PickerWidth = 360.dp

private val AuthRefused = setOf(401, 403)

sealed interface IntegrationStart {
    data object Catalogue : IntegrationStart

    data class Page(val serverId: String) : IntegrationStart
}

val LocalOpenIntegrationSettings = staticCompositionLocalOf<((IntegrationStart) -> Unit)?> { null }

sealed interface OutlineTreeState {
    data object Loading : OutlineTreeState

    data class Loaded(val nodes: List<OutlineDocumentNode>) : OutlineTreeState

    data class Failed(val refused: Boolean) : OutlineTreeState
}

enum class OutlinePhoneStage { Tree, Document }

class OutlineBrowser(val viewKey: String = "") {
    var page by mutableStateOf<OutlinePage?>(null)
    var panelOpen by mutableStateOf(false)
    var phoneStage by mutableStateOf<OutlinePhoneStage?>(null)
    val treeScroll = ScrollState(0)
    var collections by mutableStateOf<Map<String, List<OutlineCollection>>>(emptyMap())
    var failedAccounts by mutableStateOf<Map<String, OutlineTreeState.Failed>>(emptyMap())
    var serverId by mutableStateOf<String?>(null)
    var collectionId by mutableStateOf<String?>(null)
    var tree by mutableStateOf<OutlineTreeState>(OutlineTreeState.Loading)
    var expanded by mutableStateOf(emptySet<String>())
    var documentGone by mutableStateOf(false)
    var signedOut by mutableStateOf(false)
    var collapsed by mutableStateOf(false)
    var treeHeight by mutableStateOf(TreeHeight)
    var sized by mutableStateOf(false)
    var reloads by mutableIntStateOf(0)
    var seenDocuments = emptySet<String>()
    var reloadedFor: String? = null

    fun choose(server: String, collection: String?) {
        if (server != serverId || collection != collectionId) {
            tree = OutlineTreeState.Loading
            expanded = emptySet()
        }

        serverId = server
        collectionId = collection
    }

    fun expand(id: String, open: Boolean) {
        expanded = if (open) expanded + id else expanded - id
    }

    fun toggleCollapsed() {
        collapsed = !collapsed
    }

    fun resize(by: Dp, available: Dp) {
        val wanted = treeHeight + by

        if (wanted < TreeCollapsesBelow) {
            collapsed = true
            treeHeight = TreeHeight

            return
        }

        treeHeight = wanted.coerceAtMost((available - DocumentMinHeight).coerceAtLeast(TreeCollapsesBelow))
    }
}

private suspend fun <T> outlineCall(block: suspend () -> T): Result<T> = try {
    Result.success(block())
} catch (cancelled: CancellationException) {
    throw cancelled
} catch (failure: Exception) {
    Result.failure(failure)
}

private fun failureOf(failure: Throwable?) = OutlineTreeState.Failed((failure as? ApiException)?.status in AuthRefused)

@Composable
fun rememberOutlineBrowser(key: String, api: MyceliumApi, servers: List<ChatToolServer>, page: OutlinePage?, active: Boolean, browser: OutlineBrowser = remember(key) { OutlineBrowser("outline:$key") }): OutlineBrowser {
    val serverIds = servers.map { it.id }

    LaunchedEffect(page?.integrationId, page?.collectionId) {
        val server = page?.integrationId
        val collection = page?.collectionId

        if (server != null && collection != null) {
            browser.choose(server, collection)
        }
    }

    LaunchedEffect(active, serverIds, browser.reloads) {
        if (!active || serverIds.isEmpty()) {
            return@LaunchedEffect
        }

        val answers = coroutineScope { serverIds.map { id -> async { id to outlineCall { api.outlineCollections(id) } } }.awaitAll() }

        browser.collections = answers.mapNotNull { (id, answer) -> answer.getOrNull()?.let { id to it } }.toMap()
        browser.failedAccounts = answers.mapNotNull { (id, answer) -> answer.exceptionOrNull()?.let { id to failureOf(it) } }.toMap()

        if (browser.serverId !in serverIds) {
            val first = serverIds.firstOrNull { browser.collections[it]?.isNotEmpty() == true } ?: serverIds.first()

            browser.choose(first, browser.collections[first]?.firstOrNull()?.id)
        } else if (browser.collectionId == null) {
            browser.collectionId = browser.collections[browser.serverId]?.firstOrNull()?.id
        }
    }

    LaunchedEffect(active, browser.serverId, browser.collectionId, browser.failedAccounts, browser.reloads) {
        val server = browser.serverId

        if (!active || server == null) {
            return@LaunchedEffect
        }

        val collection = browser.collectionId

        if (collection == null) {
            browser.failedAccounts[server]?.let { browser.tree = it } ?: run {
                if (server in browser.collections) {
                    browser.tree = OutlineTreeState.Loaded(emptyList())
                }
            }

            return@LaunchedEffect
        }

        val answer = outlineCall { api.outlineDocuments(server, collection) }

        browser.tree = answer.fold({ OutlineTreeState.Loaded(it) }, { failureOf(it) })
    }

    LaunchedEffect(browser.tree, page?.documentId) {
        val nodes = (browser.tree as? OutlineTreeState.Loaded)?.nodes ?: return@LaunchedEffect
        val present = outlineDocumentIds(nodes)
        val document = page?.documentId
        val inThisCollection = document != null && page.collectionId == browser.collectionId

        outlineAncestors(nodes, document.orEmpty())?.let { browser.expanded += it }

        browser.documentGone = inThisCollection && document !in present && document in browser.seenDocuments
        browser.seenDocuments += present

        if (inThisCollection && document !in present && browser.reloadedFor != document) {
            browser.reloadedFor = document
            browser.reloads += 1
        }
    }

    return browser
}

@Composable
private fun OutlinePicker(browser: OutlineBrowser, servers: List<ChatToolServer>, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val account = servers.firstOrNull { it.id == browser.serverId }
    val collection = browser.collections[browser.serverId]?.firstOrNull { it.id == browser.collectionId }
    val several = servers.size > 1
    var open by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    val named = listOfNotNull(if (several) account?.name?.let { "account $it" } else null, collection?.name?.let { "collection $it" }).joinToString(", ")

    Box(modifier) {
        Row(
            Modifier
                .fillMaxWidth()
                .height(touchMin(Tokens.Size.ControlMd))
                .pressable({ open = !open }, description = "Outline $named. Change", shape = RoundedCornerShape(Tokens.Radius.Control), selected = open)
                .padding(horizontal = Tokens.Space.S1_5),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(MyIcon.BookOpen, colors.textMuted, Tokens.Size.IconSm)

            if (several && account != null) {
                Label(account.name, color = colors.textMuted, maxLines = 1)

                if (collection != null) {
                    Label("/", color = colors.textFaint, maxLines = 1)
                }
            }

            Label(collection?.name.orEmpty(), Modifier.weight(1f, fill = false), Tokens.Type.UiStrong, maxLines = 1)
            Icon(MyIcon.ChevronDown, colors.textMuted, Tokens.Size.IconSm)
        }

        if (open) {
            MenuPopup(onDismiss = { open = false; query = "" }, width = PickerWidth) {
                if (!LocalTouch.current) {
                    ListSearch(query, { query = it }, "collections", Modifier.padding(Tokens.Space.S1), placeholder = "Find a collection")
                }

                val matching = servers.map { server -> server to browser.collections[server.id].orEmpty().filter { it.name.contains(query.trim(), ignoreCase = true) } }.filter { (_, found) -> found.isNotEmpty() }

                matching.forEachIndexed { index, (server, found) ->
                    if (index > 0) {
                        Hairline(Modifier.padding(vertical = Tokens.Space.S1))
                    }

                    if (several) {
                        MenuLabel(server.name)
                    }

                    found.forEach { item ->
                        MenuItem(item.name, onClick = { browser.choose(server.id, item.id); open = false; query = "" }, checked = server.id == browser.serverId && item.id == browser.collectionId, leading = MyIcon.BookOpen)
                    }
                }
            }
        }
    }
}

@Composable
private fun OutlineMessage(title: String, detail: String, modifier: Modifier = Modifier, failed: Boolean = false, centred: Boolean = false, action: (@Composable () -> Unit)? = null) {
    val colors = MyTheme.colors

    Column(
        modifier.fillMaxWidth().padding(Tokens.Space.S4).semantics {
            if (failed) {
                liveRegion = LiveRegionMode.Polite
            }
        },
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1, Alignment.CenterVertically),
        horizontalAlignment = if (centred) Alignment.CenterHorizontally else Alignment.Start,
    ) {
        Label(title, style = Tokens.Type.Title, color = if (failed) colors.statusFailed else colors.text)
        Label(detail, style = Tokens.Type.Meta, color = colors.textMuted)

        if (action != null) {
            Box(Modifier.padding(top = Tokens.Space.S2)) { action() }
        }
    }
}

@Composable
private fun OutlineTreeBody(browser: OutlineBrowser, servers: List<ChatToolServer>, page: OutlinePage?, onSelect: (OutlinePage) -> Unit, modifier: Modifier = Modifier) {
    val account = servers.firstOrNull { it.id == browser.serverId }
    val openSettings = LocalOpenIntegrationSettings.current

    when (val tree = browser.tree) {
        OutlineTreeState.Loading -> Box(modifier)
        is OutlineTreeState.Failed -> if (tree.refused) {
            OutlineMessage("Outline refused the API key for ${account?.name.orEmpty()}", "Enter a new key on the integration page.", modifier, failed = true) {
                MyButton({ account?.let { openSettings?.invoke(IntegrationStart.Page(it.id)) } }, text = "Open ${account?.name.orEmpty()} integration", variant = ButtonVariant.Outline)
            }
        } else {
            OutlineMessage("Mycelium could not reach ${account?.name.orEmpty()}", "Check that ${account?.url.orEmpty().substringAfter("://").substringBefore('/')} is online, then try again.", modifier, failed = true) {
                MyButton({ browser.reloads += 1 }, text = "Try again", variant = ButtonVariant.Outline)
            }
        }
        is OutlineTreeState.Loaded -> if (tree.nodes.isEmpty()) {
            val name = browser.collections[browser.serverId]?.firstOrNull { it.id == browser.collectionId }?.name ?: account?.name.orEmpty()

            OutlineMessage("$name has no documents yet", "Documents added in Outline appear here.", modifier)
        } else {
            val collection = browser.collectionId.orEmpty()

            OutlineTree(tree.nodes, browser.expanded, page?.documentId, browser::expand, { node -> account?.let { onSelect(outlinePageOf(node, it, collection)) } }, modifier.verticalScrollWithBar(browser.treeScroll))
        }
    }
}

@Composable
private fun ColumnScope.OutlineDocument(browser: OutlineBrowser, platform: Platform, page: OutlinePage) {
    val colors = MyTheme.colors
    val reading = remember(browser, page.url) {
        val origin = page.url.substringBefore("://") + "://" + page.url.substringAfter("://").substringBefore('/')

        PageReading("location.pathname", { it.startsWith(origin) }) { path -> browser.signedOut = path == OutlineSignInPath }
    }

    LaunchedEffect(page.url) {
        browser.signedOut = false
    }

    Label(page.url.substringAfter("://"), Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted, maxLines = 1)

    if (browser.documentGone) {
        OutlineMessage("This document no longer exists", "It may have been deleted or moved. Choose another document in the tree.", Modifier.weight(1f))

        return
    }

    if (browser.signedOut) {
        Row(
            Modifier.fillMaxWidth().background(colors.bgRaised).padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2).semantics { liveRegion = LiveRegionMode.Polite },
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(MyIcon.Info, colors.textMuted, Tokens.Size.IconSm)
            Label("Sign in once on this device to open documents here.", style = Tokens.Type.Meta)
        }

        Hairline()
    }

    CompositionLocalProvider(LocalRetainedViewKey provides browser.viewKey.takeIf { it.isNotEmpty() }) {
        platform.WebContent(page.url, null, Modifier.weight(1f).fillMaxWidth(), reading)
    }
}

@Composable
fun OutlinePanel(browser: OutlineBrowser, platform: Platform, servers: List<ChatToolServer>, page: OutlinePage?, onSelect: (OutlinePage) -> Unit, onClose: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val back = LocalPanelBack.current
    val close = onClose.takeIf { back == null }
    val openSettings = LocalOpenIntegrationSettings.current
    val collection = browser.collections[browser.serverId]?.firstOrNull { it.id == browser.collectionId }

    BoxWithConstraints(modifier.fillMaxSize().background(colors.bgPanel).semantics { contentDescription = "Outline: ${collection?.name.orEmpty()}" }) {
        val available = maxHeight

        LaunchedEffect(Unit) {
            if (!browser.sized) {
                browser.sized = true
                browser.collapsed = available < CollapsedBelowPanelHeight
            }
        }

        Column(Modifier.fillMaxSize()) {
            if (servers.isEmpty()) {
                Bar {
                    Label("Outline", Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title, maxLines = 1)

                    if (close != null) {
                        MyButton(close, icon = MyIcon.Close, description = "Close Outline", size = ButtonSize.Sm, muted = true, tooltip = "Close")
                    }
                }

                OutlineMessage("No Outline account in this group", "Add an Outline integration, or make one available to this group.", Modifier.fillMaxSize(), centred = true) {
                    MyButton({ openSettings?.invoke(IntegrationStart.Catalogue) }, text = "Add Outline integration", variant = ButtonVariant.Primary)
                }

                return@Column
            }

            Bar(startPadding = if (back == null) Tokens.Space.S1_5 else Tokens.Space.S3, gap = if (back == null) Tokens.Space.S1 else Tokens.Space.S2) {
                if (back != null) {
                    OpenListButton()
                    MyButton(back, text = "Back to conversation", icon = MyIcon.ChevronLeft, description = "Back to the conversation", muted = true)
                }

                OutlinePicker(browser, servers, Modifier.weight(1f))

                if (platform.embedsOtherSites) {
                    MyButton(browser::toggleCollapsed, icon = if (browser.collapsed) MyIcon.ChevronDown else MyIcon.ChevronUp, description = if (browser.collapsed) "Expand the document tree" else "Collapse the document tree", size = ButtonSize.Sm, muted = true)
                }

                if (close != null) {
                    MyButton(close, icon = MyIcon.Close, description = "Close Outline", size = ButtonSize.Sm, muted = true, tooltip = "Close")
                }
            }

            if (!platform.embedsOtherSites) {
                OutlineTreeBody(browser, servers, page, onSelect, Modifier.weight(1f).fillMaxWidth())
                Hairline()

                Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Icon(MyIcon.External, colors.textMuted, Tokens.Size.IconSm)
                    Label("Outline does not allow its pages inside another site, so documents open in a new tab.", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                return@Column
            }

            if (!browser.collapsed) {
                OutlineTreeBody(browser, servers, page, onSelect, Modifier.height(browser.treeHeight).fillMaxWidth())
                ResizeHandle("Resize the document tree", Orientation.Vertical, browser::toggleCollapsed) { moved -> browser.resize(moved, available) }
            }

            if (page != null) {
                Bar {
                    Label(page.title, Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title, maxLines = 1)
                    MyButton({ platform.openUrl(page.url) }, icon = MyIcon.Globe, description = "Open in browser", size = ButtonSize.Sm, muted = true, tooltip = "Open in browser")
                }

                OutlineDocument(browser, platform, page)
            }
        }
    }
}

@Composable
fun ColumnScope.OutlinePhone(browser: OutlineBrowser, platform: Platform, servers: List<ChatToolServer>, page: OutlinePage?, stage: OutlinePhoneStage, onStage: (OutlinePhoneStage) -> Unit, onSelect: (OutlinePage) -> Unit, onClose: () -> Unit) {
    if (stage == OutlinePhoneStage.Document && page != null) {
        Bar(startPadding = Tokens.Space.S1) {
            MyButton({ onStage(OutlinePhoneStage.Tree) }, icon = MyIcon.ChevronLeft, description = "Back to the document tree", muted = true)
            BarTitle(if (browser.signedOut) servers.firstOrNull { it.id == page.integrationId }?.name ?: page.title else page.title, Modifier.weight(1f))
        }

        OutlineDocument(browser, platform, page)

        return
    }

    Bar(startPadding = Tokens.Space.S1, gap = 0.dp) {
        MyButton(onClose, icon = MyIcon.ChevronLeft, description = "Back to the conversation", muted = true)
        OutlinePicker(browser, servers, Modifier.weight(1f))
    }

    OutlineTreeBody(browser, servers, page, { chosen ->
        onSelect(chosen)

        if (platform.embedsOtherSites) {
            onStage(OutlinePhoneStage.Document)
        }
    }, Modifier.fillMaxSize())
}
