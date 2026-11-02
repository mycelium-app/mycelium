package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.Platform
import mycelium.api.AppSummary
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.addressLabel
import mycelium.state.addressUrl

fun conversationApps(state: AppState, conversationId: String): List<AppSummary> =
    state.apps.orEmpty().filter { it.conversationId == conversationId && it.address != null }

@Composable
fun AppsPanel(
    store: Store,
    state: AppState,
    platform: Platform,
    conversationId: String,
    selected: String?,
    onSelect: (String?) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = MyTheme.colors

    LaunchedEffect(conversationId) {
        store.load { store.refreshApps() }
    }

    val apps = conversationApps(state, conversationId)
    val viewed = apps.firstOrNull { it.uuid == selected }

    if (viewed?.address != null) {
        BrowserPanel(platform, addressUrl(viewed.address), viewed.name, onClose = { onSelect(null) }, modifier)

        return
    }

    Column(modifier) {
        Bar {
            PanelTitle("Apps")
        }

        if (apps.isEmpty()) {
            Label("No app was built in this conversation.", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
        } else {
            LazyColumn(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1), contentPadding = PaddingValues(Tokens.Space.S2)) {
                items(apps, key = { it.uuid }) { app ->
                    Row(
                        Modifier.fillMaxWidth().pressable({ onSelect(app.uuid) }, shape = BlockShape, description = "View ${app.name}").padding(Tokens.Space.S2),
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(MyIcon.AppWindow, colors.textMuted, Tokens.Size.IconSm)

                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                            Label(app.name, style = Tokens.Type.UiStrong, maxLines = 1)
                            Label(addressLabel(app.address.orEmpty()), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                        }
                    }
                }
            }
        }
    }
}
