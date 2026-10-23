package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.ImportResult
import mycelium.state.PickedFile
import mycelium.state.Store

internal fun importedChatsMessage(result: ImportResult): String {
    val chats = if (result.imported == 1) "1 chat" else "${result.imported} chats"

    return when (result.skipped) {
        0 -> "Imported $chats"
        1 -> "Imported $chats, 1 was already here"
        else -> "Imported $chats, ${result.skipped} were already here"
    }
}

@Composable
fun ImportFromClaude(store: Store) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current ?: return
    val scope = rememberCoroutineScope()
    var outcome by remember { mutableStateOf<String?>(null) }
    var failed by remember { mutableStateOf(false) }

    suspend fun import(files: List<PickedFile>) {
        try {
            val results = files.map { store.api.importClaudeChats(it.bytes) }

            outcome = importedChatsMessage(ImportResult(results.sumOf { it.imported }, results.sumOf { it.skipped }))
            failed = false
            store.refresh()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: ApiException) {
            outcome = failure.message
            failed = true
        } catch (_: Exception) {
            outcome = "Could not reach the control plane. Check your connection and try again."
            failed = true
        }
    }

    Column(Modifier.padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row {
            MyButton(
                { platform.pickFiles { files -> if (files.isNotEmpty()) scope.launch { import(files) } } },
                text = "Import from Claude",
                icon = MyIcon.File,
                variant = ButtonVariant.Outline,
            )
        }

        outcome?.let { Label(it, color = if (failed) colors.statusFailed else colors.textMuted, modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite }) }
    }
}
