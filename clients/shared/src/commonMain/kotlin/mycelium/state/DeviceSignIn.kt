package mycelium.state

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import mycelium.api.ApiException
import mycelium.api.DevicePoll
import mycelium.api.DeviceRequest
import mycelium.api.DeviceResponse
import mycelium.api.DeviceTokenResponse
import mycelium.api.MyceliumApi

class SignInRejected(message: String) : ApiException(0, "sign_in_rejected", message)

suspend fun signInWithDeviceCode(
    api: MyceliumApi,
    request: DeviceRequest,
    returnTo: String? = null,
    onCode: (DeviceResponse) -> Unit,
): DeviceTokenResponse {
    val device = api.startDevice(request, returnTo)
    onCode(device)

    while (true) {
        delay(device.interval * 1_000)

        val poll = try {
            api.pollDevice(device.deviceCode)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: ApiException) {
            throw failure
        } catch (_: Exception) {
            DevicePoll.Pending
        }

        when (poll) {
            DevicePoll.Pending -> Unit
            is DevicePoll.Approved -> return poll.token
            is DevicePoll.Rejected -> throw SignInRejected(rejectionSentence(poll.code))
        }
    }
}

fun rejectionSentence(code: String): String = when (code) {
    "expired_token" -> "The code expired before it was approved. Select Continue to get a new one."
    "access_denied" -> "The sign-in was denied. Select Continue to try again."
    else -> "The sign-in failed. Select Continue to try again."
}

fun normaliseAddress(input: String): String? {
    val trimmed = input.trim().trimEnd('/')

    if (trimmed.isEmpty() || trimmed.any { it.isWhitespace() }) {
        return null
    }

    return if (trimmed.startsWith("http://") || trimmed.startsWith("https://")) trimmed else "https://$trimmed"
}
