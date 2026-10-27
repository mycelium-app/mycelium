package mycelium

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.MyceliumJson
import mycelium.state.KeepState
import mycelium.state.ResponseDraft
import mycelium.state.ResponseShape
import mycelium.state.parseEdited
import mycelium.state.pathFrom
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

private fun json(text: String): JsonElement = MyceliumJson.parseToJsonElement(text)

private val messages = json("""[{"sender":"Grace","content":"Noon?"},{"sender":"Linus","content":"Pizza?"},{"sender":"Ada","content":"Count me in"},{"sender":"Bob","content":"Logs?"}]""")
private val document = json("""{"document":{"id":"d1","text":"Tag it.","createdBy":{"name":"Grace","email":"grace@example.com"}}}""")

class ResponseDraftTest {
    @Test
    fun aListOfObjectsIsATableAndAnObjectIsATreeAndTextIsText() {
        assertEquals(ResponseShape.Table, ResponseDraft(messages).shape)
        assertEquals(ResponseShape.Tree, ResponseDraft(document).shape)
        assertEquals(ResponseShape.Text, ResponseDraft(JsonPrimitive("Sent.")).shape)
        assertEquals(listOf("sender", "content"), ResponseDraft(messages).columns)
    }

    @Test
    fun droppedRowsAreLeftOutOfWhatIsSent() {
        val draft = ResponseDraft(messages).toggle(listOf(1)).toggle(listOf(3))

        assertEquals(json("""[{"sender":"Grace","content":"Noon?"},{"sender":"Ada","content":"Count me in"}]"""), draft.result())
        assertEquals(2, draft.keptRowCount())
    }

    @Test
    fun selectNoneThenOneRowKeepsJustThatRow() {
        val draft = ResponseDraft(messages).keepNoRows().toggle(listOf(2))

        assertEquals(json("""[{"sender":"Ada","content":"Count me in"}]"""), draft.result())
        assertEquals(messages, draft.keepNoRows().keepAllRows().result())
    }

    @Test
    fun shiftClickingARowAppliesTheRowsNewStateToTheWholeRange() {
        val none = ResponseDraft(messages).keepNoRows()
        val kept = none.toggleRange(from = 0, to = 2)

        assertEquals(3, kept.keptRowCount())
        assertEquals(1, kept.toggleRange(from = 2, to = 1).keptRowCount())
    }

    @Test
    fun theFilterPreselectsTheRowsItsPathsPointAt() {
        val draft = ResponseDraft(messages).select(listOf(pathFrom(listOf(JsonPrimitive(0))), pathFrom(listOf(JsonPrimitive(2), JsonPrimitive("content")))))

        assertEquals(2, draft.keptRowCount())
        assertEquals(KeepState.All, draft.keepState(listOf(0)))
        assertEquals(KeepState.None, draft.keepState(listOf(1)))
    }

    @Test
    fun droppingAFieldRemovesItAndAMixedParentShowsPartlyKept() {
        val draft = ResponseDraft(document).toggle(listOf("document", "createdBy", "email"))

        assertEquals(json("""{"document":{"id":"d1","text":"Tag it.","createdBy":{"name":"Grace"}}}"""), draft.result())
        assertEquals(KeepState.Some, draft.keepState(listOf("document", "createdBy")))
        assertEquals(KeepState.None, draft.keepState(listOf("document", "createdBy", "email")))
    }

    @Test
    fun keepingAChildOfADroppedParentKeepsOnlyThatChild() {
        val draft = ResponseDraft(document).toggle(listOf("document", "createdBy")).toggle(listOf("document", "createdBy", "name"))

        assertEquals(json("""{"document":{"id":"d1","text":"Tag it.","createdBy":{"name":"Grace"}}}"""), draft.result())
    }

    @Test
    fun anEditedFieldIsSentAsEditedAndResetRestoresIt() {
        val path = listOf<Any>("document", "text")
        val edited = ResponseDraft(document).edit(path, JsonPrimitive("Ship it."))

        assertEquals(json("""{"document":{"id":"d1","text":"Ship it.","createdBy":{"name":"Grace","email":"grace@example.com"}}}"""), edited.result())
        assertEquals(true, edited.isEdited(path))
        assertEquals(document, edited.reset(path).result())
    }

    @Test
    fun editingAFieldBackToItsOriginalValueIsNotAnEdit() {
        val path = listOf<Any>("document", "id")

        assertEquals(false, ResponseDraft(document).edit(path, JsonPrimitive("d2")).edit(path, JsonPrimitive("d1")).isEdited(path))
    }

    @Test
    fun editedTextKeepsTypesAndRefusesWhatIsNotAValue() {
        assertEquals(JsonPrimitive("12x"), parseEdited("12x", JsonPrimitive("a")))
        assertEquals(JsonPrimitive(12L), parseEdited("12", JsonPrimitive(1L)))
        assertEquals(JsonPrimitive(true), parseEdited("true", JsonPrimitive(false)))
        assertNull(parseEdited("twelve", JsonPrimitive(1L)))
    }
}
