package mycelium

import mycelium.state.EditHistory
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class EditHistoryTest {
    private fun typed(text: String): Pair<EditHistory<String>, String> =
        text.fold(EditHistory<String>() to "") { (history, current), char -> history.recorded(current, typedOneCharacter = true, typedSpace = char == ' ') to current + char }

    @Test
    fun typingIsUndoneAWordAtATime() {
        val (history, current) = typed("one two")

        val (afterFirst, back) = history.undone(current)!!
        assertEquals("one ", back)
        assertEquals("", afterFirst.undone(back)!!.second)
    }

    @Test
    fun aPasteIsItsOwnStepAndRedoReplaysWhatWasUndone() {
        val (typedHistory, typed) = typed("see ")
        val history = typedHistory.recorded(typed, typedOneCharacter = false, typedSpace = false)

        val (undone, back) = history.undone("see <paste>")!!
        assertEquals("see ", back)
        assertEquals("see <paste>", undone.redone(back)!!.second)
    }

    @Test
    fun aNewEditAfterUndoDropsTheRedo() {
        val (history, current) = typed("ab")
        val (undone, back) = history.undone(current)!!

        assertNull(undone.recorded(back, typedOneCharacter = false, typedSpace = false).redone("x"))
    }
}
