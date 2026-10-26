package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import mycelium.state.ThreadItem

class ReviewActions(val load: suspend (requestId: String) -> List<JsonElement>?, val answer: suspend (requestId: String, decision: String, removed: List<Int>) -> Unit)

val LocalReviewActions = compositionLocalOf<ReviewActions?> { null }

private const val Share = "share"
private const val Remove = "remove"
private const val Withhold = "withhold"

internal fun reviewSummary(item: JsonElement): String {
    val message = item as? JsonObject ?: return (item as? JsonPrimitive)?.contentOrNull ?: item.toString()
    val sender = (message["sender"] as? JsonPrimitive)?.contentOrNull
    val place = (message["place"] as? JsonObject)?.values?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }?.joinToString(" / ")
    val content = (message["content"] as? JsonPrimitive)?.contentOrNull

    if (content == null) {
        return message.toString()
    }

    return listOfNotNull(sender, place?.takeIf { it.isNotEmpty() }).joinToString(", ").let { head -> if (head.isEmpty()) content else "$head: $content" }
}

private fun resolvedText(review: ThreadItem.Review) = when (review.decision) {
    Share -> "Shared with the agent"
    Remove -> "Shared with the agent, ${review.removedCount} of ${review.count} removed"
    else -> "Withheld from the agent"
}

@Composable
internal fun ReviewCard(review: ThreadItem.Review) {
    val actions = LocalReviewActions.current
    val scope = rememberCoroutineScope()
    val loaded by produceState<List<JsonElement>?>(null, review.requestId, review.resolved, actions) {
        value = if (review.resolved) null else actions?.load?.invoke(review.requestId)
    }
    var removed by remember(review.requestId) { mutableStateOf(setOf<Int>()) }
    var expanded by remember(review.requestId) { mutableStateOf(setOf<Int>()) }
    var problem by remember(review.requestId) { mutableStateOf<String?>(null) }
    val items = loaded.orEmpty()
    val many = review.count > 1

    fun answer(decision: String) {
        if (decision == Remove && removed.isEmpty()) {
            problem = "Untick the items to remove."

            return
        }

        problem = null
        scope.launch { actions?.answer?.invoke(review.requestId, decision, removed.sorted()) }
    }

    Card {
        CardHead {
            StatusMark(if (review.resolved) resolvedText(review) else "Waiting for your review", if (review.resolved) Tone.Muted else Tone.Attention, if (review.resolved) MyIcon.Check else MyIcon.Alert)
        }

        Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("${review.server}: ${review.tool}", style = Tokens.Type.Code, maxLines = 1)

            if (!review.resolved) {
                Label("The agent has not seen this result yet.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
            }

            if (!review.resolved && loaded == null) {
                Label("This result is no longer waiting for review.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
            }

            for ((index, item) in items.withIndex()) {
                val text = reviewSummary(item)
                val open = index in expanded
                val number = index + 1

                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(
                        checked = index !in removed,
                        description = "Share item $number with the agent",
                        onToggle = {
                            removed = if (index in removed) removed - index else removed + index
                            problem = null
                        },
                        checkedState = "Shared",
                        uncheckedState = "Removed",
                    )

                    Label(
                        text,
                        Modifier.weight(1f).pressable(
                            onClick = { expanded = if (open) expanded - index else expanded + index },
                            role = Role.Button,
                            description = if (open) "Show less of item $number" else "Show all of item $number",
                            stateDescription = if (open) "Expanded" else "Collapsed",
                        ),
                        Tokens.Type.Code,
                        maxLines = if (open) Int.MAX_VALUE else 1,
                    )
                }
            }

            if (problem != null) {
                StatusMark(problem.orEmpty(), Tone.Failed, MyIcon.Failed)
            }
        }

        if (!review.resolved && loaded != null) {
            Hairline()

            Row(
                Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End),
            ) {
                MyButton({ answer(Withhold) }, text = "Withhold", variant = ButtonVariant.Outline, size = ButtonSize.Md, description = "Withhold this result from the agent")

                if (many) {
                    MyButton({ answer(Remove) }, text = if (removed.isEmpty()) "Remove items" else "Remove ${removed.size} ${if (removed.size == 1) "item" else "items"}", variant = ButtonVariant.Outline, size = ButtonSize.Md, description = "Share the result without the unticked items")
                }

                MyButton({ answer(Share) }, text = "Share", variant = ButtonVariant.Primary, size = ButtonSize.Md, description = "Share the whole result with the agent")
            }
        }
    }
}
