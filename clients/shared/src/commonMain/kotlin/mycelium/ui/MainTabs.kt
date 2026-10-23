package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.isTertiaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.CancellationException
import mycelium.api.Conversation
import mycelium.state.DiffTab
import mycelium.state.FileDiff
import mycelium.state.Store
import mycelium.state.ToolStatus
import mycelium.state.parseUnifiedDiff
import mycelium.state.splitCommitDiff

class MainTab(val key: String, val title: String, val closeLabel: String? = null)

@Composable
fun MainTabStrip(tabs: List<MainTab>, selected: String, onSelect: (String) -> Unit, onClose: (String) -> Unit, groupLabel: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Row(modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).semantics { contentDescription = groupLabel }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        for (tab in tabs) {
            val on = tab.key == selected
            val closing = rememberUpdatedState { tab.closeLabel?.let { onClose(tab.key) } }

            Row(Modifier.pointerInput(tab.key) {
                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent(PointerEventPass.Initial)

                        if (event.type == PointerEventType.Press && event.buttons.isTertiaryPressed) {
                            closing.value()
                            event.changes.forEach { it.consume() }
                        }
                    }
                }
            }, verticalAlignment = Alignment.CenterVertically) {
                Column(
                    Modifier
                        .heightIn(min = rowHeight())
                        .pressable({ onSelect(tab.key) }, role = Role.Tab, shape = ControlShape, ringInside = true)
                        .semantics { this.selected = on }
                        .padding(horizontal = Tokens.Space.S2),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Label(tab.title, style = if (on) Tokens.Type.UiStrong else Tokens.Type.Ui, color = if (on) colors.text else colors.textMuted, maxLines = 1)
                    Box(Modifier.width(Tokens.Space.S6).height(Tokens.Size.FocusRing).background(if (on) colors.text else Color.Transparent))
                }

                if (tab.closeLabel != null) {
                    MyButton({ onClose(tab.key) }, icon = MyIcon.Close, description = tab.closeLabel, size = ButtonSize.Sm, muted = true)
                }
            }
        }
    }
}

@Composable
fun DiffTabView(store: Store, conversation: Conversation, tab: DiffTab, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    var loaded by remember(tab.key) { mutableStateOf<List<FileDiff>?>(null) }
    var truncated by remember(tab.key) { mutableStateOf(false) }
    var failed by remember(tab.key) { mutableStateOf(false) }

    LaunchedEffect(tab.key, conversation.id) {
        failed = false

        try {
            if (tab.hash != null) {
                val shown = store.api.gitShow(conversation.id, tab.hash)
                loaded = splitCommitDiff(shown.diff)
                truncated = shown.truncated
            } else if (tab.path != null) {
                val shown = store.api.gitDiff(conversation.id, tab.path, tab.staged)
                loaded = if (shown.diff.isBlank()) emptyList() else listOf(parseUnifiedDiff(tab.path, shown.diff))
                truncated = shown.truncated
            }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            failed = true
        }
    }

    Column(modifier.fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        val files = loaded

        when {
            failed -> Label("Could not load this diff. Close the tab and open it again.", color = colors.textMuted)
            files == null -> Label("Loading the diff", color = colors.textMuted)
            files.isEmpty() -> Label("There are no text changes to show.", color = colors.textMuted)
            else -> {
                for (file in files) {
                    DiffCard(file, ToolStatus.Done)
                }

                if (truncated) {
                    Label("The diff is cut at 200 KB.", style = Tokens.Type.Meta, color = colors.textMuted)
                }
            }
        }
    }
}
