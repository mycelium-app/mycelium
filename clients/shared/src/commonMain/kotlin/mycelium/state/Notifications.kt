package mycelium.state

import mycelium.api.ConversationStatus

const val NotifyFinishedPreference = "notify-finished"
const val NotifySoundPreference = "notify-sound"

fun finishedWording(status: ConversationStatus): String? = when (status) {
    ConversationStatus.Done -> "Done"
    ConversationStatus.Input -> "Needs your input"
    ConversationStatus.Review -> "Ready to review"
    ConversationStatus.Failed -> "Failed"
    ConversationStatus.Working -> null
}
