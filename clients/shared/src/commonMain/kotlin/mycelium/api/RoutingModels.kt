package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

object Routing {
    const val ClaudeAny = "claude_any"
    const val CodexAny = "codex_any"
    const val AnyModel = "any_model"
}

@Serializable
data class DefaultModes(val claude: String? = null, val codex: String? = null)

@Serializable
data class AccountMode(val agent: String, val account: String, val mode: String)

@Serializable
data class AgentSettings(
    @SerialName("default_modes") val defaultModes: DefaultModes = DefaultModes(),
    @SerialName("account_modes") val accountModes: List<AccountMode> = emptyList(),
    @SerialName("any_model") val anyModel: List<ModelChoice> = emptyList(),
    @SerialName("node_preferences") val nodePreferences: Map<String, String> = emptyMap(),
)

@Serializable
data class SaveAgentSettingsRequest(
    @SerialName("default_modes") val defaultModes: DefaultModes = DefaultModes(),
    @SerialName("any_model") val anyModel: List<ModelChoice> = emptyList(),
    @SerialName("node_preferences") val nodePreferences: Map<String, String> = emptyMap(),
)

object NodePreference {
    const val Preferred = "preferred"
    const val Normal = "normal"
    const val Avoid = "avoid"
    const val Never = "never"
}

const val AnyNodeId = "00000000-0000-0000-0000-000000000000"

@Serializable
data class AnyNodePick(@SerialName("node_id") val nodeId: String, val name: String)

@Serializable
data class SetAccountModeRequest(val agent: String, val account: String, val mode: String? = null)

@Serializable
data class Price(val amount: Double, val currency: String, val period: String)

@Serializable
data class AccountPrice(val agent: String, val account: String, val amount: Double, val currency: String, val period: String)

@Serializable
data class SetAccountPriceRequest(val agent: String, val account: String, val price: Price? = null)

@Serializable
data class YourPrice(
    val amount: Double,
    val currency: String,
    val period: String,
    @SerialName("monthly_amount") val monthlyAmount: Double,
    @SerialName("monthly_usd") val monthlyUsd: Double? = null,
    @SerialName("rate_date") val rateDate: String? = null,
)
