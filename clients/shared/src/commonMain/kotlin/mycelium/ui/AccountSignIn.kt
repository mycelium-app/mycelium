package mycelium.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import mycelium.Platform
import mycelium.state.Store

val LocalPlatform = compositionLocalOf<Platform?> { null }

@Composable
fun AccountSignInDialog(store: Store, session: LoginSession, onClose: () -> Unit) {
    val state by store.state.collectAsState()
    val platform = LocalPlatform.current
    val finished = state.finishedLogins[session.loginId]

    Dialog(onDismiss = onClose) {
        LoginPanel(
            session = session,
            output = state.loginOutput[session.loginId].orEmpty(),
            finished = finished,
            onOpenUrl = { url -> platform?.openUrl(url) },
            onCopy = { code -> platform?.copyToClipboard(code) },
            onInput = { text -> store.act { store.api.loginInput(session.loginId, text) } },
            framed = false,
            onClose = {
                if (finished == null) {
                    store.act { store.api.stopLogin(session.loginId) }
                }

                onClose()
            },
        )
    }
}
