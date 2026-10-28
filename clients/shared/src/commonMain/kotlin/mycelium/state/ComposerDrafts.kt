package mycelium.state

data class ComposerDraft(
    val text: String = "",
    val cursor: Int = 0,
    val images: Map<Char, Int> = emptyMap(),
    val annotations: List<Annotation> = emptyList(),
    val attachments: List<DraftAttachment> = emptyList(),
)

class ComposerDrafts {
    private val held = mutableMapOf<String, ComposerDraft>()

    operator fun get(key: String): ComposerDraft = held[key] ?: ComposerDraft()

    fun update(key: String, change: ComposerDraft.() -> ComposerDraft) {
        val updated = get(key).change()

        if (updated == ComposerDraft()) {
            held.remove(key)
        } else {
            held[key] = updated
        }
    }
}
