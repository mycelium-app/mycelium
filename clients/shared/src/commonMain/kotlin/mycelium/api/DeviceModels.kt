package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
enum class DeviceKind {
    @SerialName("node")
    Node,

    @SerialName("client")
    Client,
}

@Serializable
data class Me(val id: String, val email: String, val name: String)

@Serializable
data class DeviceRequest(
    val kind: DeviceKind,
    val label: String,
    val hostname: String,
    val os: String,
    val arch: String,
)

@Serializable
data class DeviceResponse(
    @SerialName("device_code") val deviceCode: String,
    @SerialName("user_code") val userCode: String,
    @SerialName("verification_uri") val verificationUri: String,
    @SerialName("verification_uri_complete") val verificationUriComplete: String,
    val interval: Long,
    @SerialName("expires_in") val expiresIn: Long,
)

@Serializable
data class DeviceTokenRequest(@SerialName("device_code") val deviceCode: String)

@Serializable
data class DeviceTokenResponse(val kind: DeviceKind, val id: String, val token: String)

@Serializable
data class PendingDevice(
    @SerialName("user_code") val userCode: String,
    val kind: DeviceKind,
    val label: String,
    val hostname: String,
    val os: String,
    val arch: String,
    val ip: String,
    @SerialName("expires_at") val expiresAt: String,
)
