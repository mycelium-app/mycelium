package mycelium

import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.pointerInput
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ApproveScreen
import mycelium.ui.Label
import mycelium.ui.LocalKeyboardNavigation
import mycelium.ui.LocalReducedMotion
import mycelium.ui.LocalTouch
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.Shell
import mycelium.ui.SignInScreen
import mycelium.ui.Tokens

private sealed interface Phase {
    data object Restoring : Phase

    data class SignedOut(val notice: String?) : Phase

    data class SignedIn(val api: MyceliumApi, val me: Me) : Phase
}

@Composable
fun App(platform: Platform, dark: Boolean? = null) {
    if (dark == null) {
        MyceliumTheme { AppContent(platform) }
    } else {
        MyceliumTheme(dark) { AppContent(platform) }
    }
}

@Composable
private fun AppContent(platform: Platform) {
    val scope = rememberCoroutineScope()
    val saved = remember { platform.loadConnection() }
    var phase by remember { mutableStateOf<Phase>(Phase.Restoring) }
    var approveCode by remember { mutableStateOf(platform.approveCode()) }
    var keyboardNavigation by remember { mutableStateOf(false) }
    val root = remember { FocusRequester() }

    LaunchedEffect(Unit) {
        val baseUrl = saved?.baseUrl ?: platform.originUrl

        if (baseUrl == null) {
            phase = Phase.SignedOut(null)
            return@LaunchedEffect
        }

        val api = MyceliumApi(baseUrl, saved?.token)

        phase = try {
            Phase.SignedIn(api, api.me())
        } catch (failure: ApiException) {
            if (failure.status == 401) {
                platform.saveConnection(null)
                Phase.SignedOut(null)
            } else {
                Phase.SignedOut(failure.message)
            }
        } catch (_: Exception) {
            Phase.SignedOut("Could not reach $baseUrl. Check your connection and try again.")
        }
    }

    BoxWithConstraints(
        Modifier
            .fillMaxSize()
            .background(MyTheme.colors.bgContent)
            .windowInsetsPadding(WindowInsets.safeDrawing)
            .onPreviewKeyEvent { event ->
                if (event.type == KeyEventType.KeyDown) {
                    keyboardNavigation = true
                }

                false
            }
            .pointerInput(Unit) {
                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent(PointerEventPass.Initial)

                        if (event.type == PointerEventType.Press) {
                            keyboardNavigation = false
                        }
                    }
                }
            }
            .pointerInput(Unit) { detectTapGestures { root.requestFocus() } }
            .focusRequester(root)
            .focusable(),
        contentAlignment = Alignment.TopStart,
    ) {
        val phone = maxWidth < Tokens.Size.Compact

        CompositionLocalProvider(LocalTouch provides phone, LocalKeyboardNavigation provides keyboardNavigation, LocalReducedMotion provides platform.reducedMotion) {
            when (val current = phase) {
                Phase.Restoring -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Label("Loading", color = MyTheme.colors.textMuted)
                }

                is Phase.SignedOut -> SignInScreen(platform, saved?.baseUrl, current.notice) { api, me -> phase = Phase.SignedIn(api, me) }

                is Phase.SignedIn -> {
                    val code = approveCode

                    if (code != null) {
                        ApproveScreen(current.api, code, onDone = { approveCode = null })
                    } else {
                        val store = remember(current.api) { Store(current.api, scope) }

                        DisposableEffect(store) {
                            store.start()
                            onDispose { store.stop() }
                        }

                        Shell(
                            store = store,
                            platform = platform,
                            me = current.me,
                            onSignOut = {
                                scope.launch {
                                    runCatching { current.api.logout() }
                                    platform.saveConnection(null)
                                    phase = Phase.SignedOut(null)
                                }
                            },
                        )
                    }
                }
            }
        }
    }
}
