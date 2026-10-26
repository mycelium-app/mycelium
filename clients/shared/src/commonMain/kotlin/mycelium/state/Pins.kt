package mycelium.state

import kotlin.time.Clock
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant

const val MaxPinned = 3

private val RunningWorkflowQuiet = 30.minutes
private val BetweenWorkflowStages = 2.minutes

data class PinnedCard(val key: String, val label: String, val ui: UiNode? = null, val workflow: WorkflowProgress? = null)

fun workflowPinKey(runId: String): String = "workflow:$runId"

fun pinnedCards(items: List<ThreadItem>, dismissed: Set<String>, now: Instant = Clock.System.now()): List<PinnedCard> {
    val pinned = LinkedHashMap<String, PinnedCard>()

    for (item in items) {
        when (item) {
            is ThreadItem.AgentMessage -> for (block in parsedMarkdown(item.text)) {
                if (block !is MarkdownBlock.Code || block.language != GeneratedUiLanguage) {
                    continue
                }

                val ui = parseUiBlock(block.text) ?: continue

                if (ui.sticky || pinned.containsKey(ui.pinKey)) {
                    pinned[ui.pinKey] = PinnedCard(ui.pinKey, ui.label, ui = ui.node)
                }
            }

            is ThreadItem.Workflow -> {
                val key = workflowPinKey(item.progress.runId)

                val sinceLastLine = item.progress.lastLineAt?.let { now - it }
                val quietLimit = if (item.progress.complete) BetweenWorkflowStages else RunningWorkflowQuiet

                if (sinceLastLine != null && sinceLastLine < quietLimit) {
                    pinned[key] = PinnedCard(key, "Workflow", workflow = item.progress)
                } else {
                    pinned.remove(key)
                }
            }

            else -> Unit
        }
    }

    return pinned.values.toList().takeLast(MaxPinned).filter { it.key !in dismissed }
}

class PinDismissals {
    private val held = mutableMapOf<String, Set<String>>()

    operator fun get(conversationId: String): Set<String> = held[conversationId].orEmpty()

    fun dismiss(conversationId: String, key: String) {
        held[conversationId] = get(conversationId) + key
    }
}
