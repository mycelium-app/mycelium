package mycelium

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
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
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ApproveScreen
import mycelium.ui.Label
import mycelium.ui.LocalReducedMotion
import mycelium.ui.LocalTouch
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.Accent
import mycelium.ui.Appearance
import mycelium.ui.LocalAppearance
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.ui.graphics.Color
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
    val systemDark = isSystemInDarkTheme()
    var chosenDark by remember { mutableStateOf(platform.loadPreference(ThemePreference)?.let { it == "dark" }) }
    val night = dark ?: chosenDark ?: systemDark
    val systemAccent = platform.systemAccent(night)?.let { Color(it.toInt()) }
    val accents = Accent.entries.filter { it != Accent.System || systemAccent != null }
    var accent by remember { mutableStateOf(accents.firstOrNull { it.name == platform.loadPreference(AccentPreference) } ?: accents.first()) }
    val appearance = Appearance(
        dark = night,
        accent = accent,
        accents = accents,
        onAccent = { chosen ->
            accent = chosen
            platform.savePreference(AccentPreference, chosen.name)
        },
        onToggleDark = {
            chosenDark = !night
            platform.savePreference(ThemePreference, if (night) "light" else "dark")
        },
    )

    CompositionLocalProvider(LocalAppearance provides appearance) {
        MyceliumTheme(night, accent, systemAccent) { AppContent(platform) }
    }
}

private const val AccentPreference = "accent"

private const val ThemePreference = "theme"

@Composable
private fun AppContent(platform: Platform) {
    val scope = rememberCoroutineScope()
    val saved = remember { platform.loadConnection() }
    var phase by remember { mutableStateOf<Phase>(Phase.Restoring) }
    var approveCode by remember { mutableStateOf(platform.approveCode()) }
    val focusManager = LocalFocusManager.current

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
            .pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Final)
                    val up = waitForUpOrCancellation(PointerEventPass.Final)

                    if (down.type == PointerType.Mouse && !down.isConsumed && up != null && !up.isConsumed) {
                        focusManager.clearFocus()
                    }
                }
            },
        contentAlignment = Alignment.TopStart,
    ) {
        val phone = maxWidth < Tokens.Size.Compact

        CompositionLocalProvider(LocalTouch provides phone, LocalReducedMotion provides platform.reducedMotion) {
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

                        LaunchedEffect(store) {
                            platform.resumes.collect { store.reconnect() }
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
