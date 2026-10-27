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
)

@Serializable
data class SaveAgentSettingsRequest(
    @SerialName("default_modes") val defaultModes: DefaultModes = DefaultModes(),
    @SerialName("any_model") val anyModel: List<ModelChoice> = emptyList(),
)

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
