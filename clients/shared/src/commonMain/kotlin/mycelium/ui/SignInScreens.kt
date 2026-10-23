package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import mycelium.Connection
import mycelium.Platform
import mycelium.api.ApiException
import mycelium.api.DeviceKind
import mycelium.api.DeviceRequest
import mycelium.api.DeviceResponse
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.api.PendingDevice
import mycelium.state.SignInRejected
import mycelium.state.normaliseAddress
import mycelium.state.signInCallbackFrom
import mycelium.state.signInWithDeviceCode

@Composable
fun SignInScreen(platform: Platform, savedAddress: String?, notice: String?, onSignedIn: (MyceliumApi, Me) -> Unit) {
    val scope = rememberCoroutineScope()
    var address by remember { mutableStateOf(savedAddress.orEmpty()) }
    var error by remember { mutableStateOf(notice) }
    var device by remember { mutableStateOf<DeviceResponse?>(null) }
    var job by remember { mutableStateOf<Job?>(null) }

    fun beginInBrowser() {
        val origin = platform.originUrl ?: return
        platform.navigate("$origin/auth/login?next=${encodeQuery(platform.currentPath())}")
    }

    fun cancelSignIn() {
        job?.cancel()
        job = null
        device = null
        error = "Sign-in was cancelled. Select Continue to try again."
    }

    fun openSignInPage(code: DeviceResponse) = platform.openSignIn(code.verificationUriComplete, ::cancelSignIn)

    fun beginWithDeviceCode() {
        val normalised = normaliseAddress(address)

        if (normalised == null) {
            error = "Enter the address of your control plane, for example mycelium.example.com."
            return
        }

        error = null
        job = scope.launch {
            try {
                val api = MyceliumApi(normalised)
                val info = platform.device()
                val request = DeviceRequest(DeviceKind.Client, info.label, info.hostname, info.os, info.arch)
                val token = signInWithDeviceCode(api, request, platform.signInCallback) {
                    device = it
                    openSignInPage(it)
                }
                val signedIn = MyceliumApi(normalised, token.token)
                val me = signedIn.me()

                platform.saveConnection(Connection(normalised, token.token))
                onSignedIn(signedIn, me)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (rejected: SignInRejected) {
                error = rejected.message
            } catch (failure: ApiException) {
                error = failure.message
            } catch (_: Exception) {
                error = "Could not reach $normalised. Check the address and your connection, then try again."
            }

            device = null
            job = null
        }
    }

    CenteredPage {
        Label("Mycelium", Modifier.semantics { heading() }, Tokens.Type.Display)

        Column(Modifier.widthIn(max = Tokens.Size.FormColumn).fillMaxWidth().padding(horizontal = Tokens.Space.S4), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            val code = device

            if (code != null) {
                DeviceCodePanel(code, platform, onOpen = { openSignInPage(code) }, onCancel = { job?.cancel(); job = null; device = null })
            } else {
                if (!platform.signsInThroughTheBrowser) {
                    TextInput(
                        address,
                        { address = it },
                        label = "Control plane address",
                        placeholder = "mycelium.example.com",
                        error = error,
                        large = true,
                        onSubmit = ::beginWithDeviceCode,
                    )
                } else if (error != null) {
                    Label(error.orEmpty(), style = Tokens.Type.MetaStrong, color = MyTheme.colors.statusFailed)
                }

                MyButton(
                    onClick = if (platform.signsInThroughTheBrowser) ::beginInBrowser else ::beginWithDeviceCode,
                    text = if (platform.signsInThroughTheBrowser) "Sign in" else "Continue",
                    variant = ButtonVariant.Primary,
                    size = ButtonSize.Lg,
                    modifier = Modifier.padding(top = Tokens.Space.S2),
                )
            }
        }
    }
}

@Composable
private fun DeviceCodePanel(device: DeviceResponse, platform: Platform, onOpen: () -> Unit, onCancel: () -> Unit) {
    val colors = MyTheme.colors

    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("Enter this code on the sign-in page", style = Tokens.Type.MetaStrong, color = colors.textMuted)

        Row(
            Modifier
                .fillMaxWidth()
                .background(colors.bgPanel, ControlShape)
                .border(Tokens.Size.Hairline, colors.border, ControlShape)
                .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Label(device.userCode, Modifier.weight(1f), Tokens.Type.CodeLarge)
            MyButton({ platform.copyToClipboard(device.userCode) }, text = "Copy", description = "Copy code ${device.userCode}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        MyButton(onOpen, text = "Open the sign-in page", variant = ButtonVariant.Primary, size = ButtonSize.Lg)

        StatusMark("Waiting for you to sign in", Tone.Working, MyIcon.Working)
        MyButton(onCancel, text = "Cancel", size = ButtonSize.Sm, muted = true)
    }
}

@Composable
fun ApproveScreen(api: MyceliumApi, code: String, returnTo: String?, onReturn: (String) -> Unit, onDone: () -> Unit, onClose: () -> Unit = {}) {
    val scope = rememberCoroutineScope()
    var pending by remember { mutableStateOf<PendingDevice?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var outcome by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(code) {
        try {
            pending = api.pendingDevice(code)
        } catch (failure: ApiException) {
            error = failure.message
        } catch (_: Exception) {
            error = "Could not reach the control plane. Check your connection and reload the page."
        }
    }

    fun answer(approve: Boolean) {
        scope.launch {
            try {
                if (approve) api.approveDevice(code) else api.denyDevice(code)
                val signingInApp = pending?.kind == DeviceKind.Client
                outcome = when {
                    !approve -> "Denied. The machine will not be connected."
                    signingInApp -> "You can close this tab and return to Mycelium."
                    else -> "Approved. You can close this page and go back to the machine."
                }
                signInCallbackFrom(returnTo)?.let(onReturn)

                if (approve && signingInApp) {
                    onClose()
                }
            } catch (failure: ApiException) {
                error = failure.message
            } catch (_: Exception) {
                error = "Could not reach the control plane. Check your connection and try again."
            }
        }
    }

    CenteredPage {
        val device = pending
        val colors = MyTheme.colors

        Column(Modifier.widthIn(max = Tokens.Size.ApproveColumn).fillMaxWidth().padding(horizontal = Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
            when {
                outcome != null -> {
                    Label("Done", Modifier.semantics { heading() }, Tokens.Type.Display)
                    Label(outcome.orEmpty())
                    MyButton(onDone, text = "Go to conversations", variant = ButtonVariant.Outline, size = ButtonSize.Lg)
                }

                error != null && device == null -> {
                    Label("Cannot approve this code", Modifier.semantics { heading() }, Tokens.Type.Display)
                    Label(error.orEmpty(), color = colors.statusFailed)
                    MyButton(onDone, text = "Go to conversations", variant = ButtonVariant.Outline, size = ButtonSize.Lg)
                }

                device == null -> Label("Loading the request", color = colors.textMuted)

                else -> {
                    val subject = if (device.kind == DeviceKind.Node) device.label else "${device.label} (desktop app)"

                    Label("Approve $subject?", Modifier.semantics { heading() }, Tokens.Type.Display)

                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Label("Check this code matches the one on the machine", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                        Label(device.userCode, style = Tokens.Type.CodeLarge)
                    }

                    Column(Modifier.fillMaxWidth()) {
                        Hairline()

                        for ((name, value) in listOf("Host" to device.hostname, "System" to "${device.os} ${device.arch}", "Address" to device.ip)) {
                            Row(Modifier.fillMaxWidth().heightIn(min = rowHeight()), verticalAlignment = Alignment.CenterVertically) {
                                Label(name, Modifier.width(Tokens.Size.ActionColumn / 2), color = colors.textMuted)
                                Label(value)
                            }

                            Hairline()
                        }
                    }

                    error?.let { Label(it, color = colors.statusFailed, style = Tokens.Type.MetaStrong) }

                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        MyButton({ answer(true) }, text = "Approve $subject", variant = ButtonVariant.Primary, size = ButtonSize.Lg)
                        MyButton({ answer(false) }, text = "Deny", variant = ButtonVariant.Outline, size = ButtonSize.Lg)
                    }
                }
            }
        }
    }
}

@Composable
private fun CenteredPage(content: @Composable () -> Unit) {
    Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
            content()
        }
    }
}

fun encodeQuery(value: String): String = buildString {
    for (char in value) {
        if (char.isLetterOrDigit() || char in "-._~") {
            append(char)
        } else {
            for (byte in char.toString().encodeToByteArray()) {
                append('%')
                append((byte.toInt() and 0xFF).toString(16).uppercase().padStart(2, '0'))
            }
        }
    }
}
