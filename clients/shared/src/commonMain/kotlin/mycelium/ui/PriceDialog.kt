package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.api.AccountPrice
import mycelium.api.Price
import mycelium.api.SetAccountPriceRequest
import mycelium.state.AmountProblem
import mycelium.state.Choice
import mycelium.state.PriceCurrencies
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.currencyName
import mycelium.state.formatAmount
import mycelium.state.parseAmount

private const val Monthly = "Monthly"

private const val Yearly = "Yearly"

@Composable
fun PriceDialog(store: Store, agent: String, account: String, current: AccountPrice?, listPrice: String?, onSaved: () -> Unit, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    var currency by remember(account) { mutableStateOf(current?.currency ?: PriceCurrencies.first().first) }
    var amount by remember(account) { mutableStateOf(current?.let { formatAmount(it.amount) }.orEmpty()) }
    var billed by remember(account) { mutableStateOf(if (current?.period == "yearly") Yearly else Monthly) }
    var amountProblem by remember(account) { mutableStateOf<String?>(null) }
    val amountFocus = remember { FocusRequester() }
    val runner = rememberAction(account)
    val fallback = listPrice?.let { "Usage history uses this instead of the $it." } ?: "Usage history uses this instead of the published list price."

    fun save() {
        val parsed = parseAmount(amount)

        if (parsed == null) {
            amountProblem = AmountProblem

            return
        }

        amountProblem = null
        runner {
            store.api.setAccountPrice(SetAccountPriceRequest(agent, account, Price(parsed, currency, if (billed == Yearly) "yearly" else "monthly")))
            onSaved()
            onDismiss()
        }
    }

    Dialog(onDismiss) {
        Label("Your price for $account, ${agentName(agent)}", Modifier.semantics { heading() }, Tokens.Type.Title)

        amountProblem?.let { problem ->
            Column(
                Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.statusFailed, ControlShape).padding(Tokens.Space.S3).semantics { liveRegion = LiveRegionMode.Assertive },
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            ) {
                Label("There is a problem", Modifier.semantics { heading() }, Tokens.Type.UiStrong, colors.statusFailed)
                MyButton({ runCatching { amountFocus.requestFocus() } }, text = problem, variant = ButtonVariant.Ghost, description = "Go to Amount: $problem")
            }
        }

        Label(fallback, style = Tokens.Type.Meta, color = colors.textMuted)

        OptionMenu("Currency", PriceCurrencies.map { Choice(it.first, currencyName(it.first)) }, currency, { currency = it }, fill = true)
        TextInput(amount, { amount = it }, label = "Amount", error = amountProblem, onSubmit = ::save, focusRequester = amountFocus)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Billed", style = Tokens.Type.MetaStrong, color = colors.textMuted)
            Segmented(listOf(Monthly, Yearly), billed, { billed = it }, groupLabel = "Billed")
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton({ runner { store.api.setAccountPrice(SetAccountPriceRequest(agent, account)); onSaved(); onDismiss() } }, text = "Use the list price", muted = true)
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}
