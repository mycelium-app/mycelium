package mycelium.state

private const val EditHistoryLimit = 200

data class EditHistory<T>(val undos: List<T> = emptyList(), val redos: List<T> = emptyList(), val typing: Boolean = false) {
    fun recorded(before: T, typedOneCharacter: Boolean, typedSpace: Boolean): EditHistory<T> {
        val joinsTyping = typing && typedOneCharacter
        val undos = if (joinsTyping) undos else (undos + before).takeLast(EditHistoryLimit)

        return EditHistory(undos, emptyList(), typedOneCharacter && !typedSpace)
    }

    fun undone(current: T): Pair<EditHistory<T>, T>? {
        val previous = undos.lastOrNull() ?: return null

        return EditHistory(undos.dropLast(1), redos + current) to previous
    }

    fun redone(current: T): Pair<EditHistory<T>, T>? {
        val next = redos.lastOrNull() ?: return null

        return EditHistory(undos + current, redos.dropLast(1)) to next
    }
}
