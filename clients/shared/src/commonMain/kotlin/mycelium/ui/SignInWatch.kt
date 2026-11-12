package mycelium.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.rememberUpdatedState
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import mycelium.api.OAuthSignInStatus
import mycelium.state.Store

private const val SignInPollMillis = 2_000L

@Composable
fun WatchSignIn(store: Store, signIn: String?, onSettled: (OAuthSignInStatus?) -> Unit) {
    val settled = rememberUpdatedState(onSettled)

    LaunchedEffect(signIn) {
        if (signIn == null) {
            return@LaunchedEffect
        }

        while (true) {
            delay(SignInPollMillis)

            val status = try {
                store.api.chatToolServerSignIn(signIn)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                OAuthSignInStatus("waiting")
            }

            if (status == null || status.state != "waiting") {
                settled.value(status)

                return@LaunchedEffect
            }
        }
    }
}
