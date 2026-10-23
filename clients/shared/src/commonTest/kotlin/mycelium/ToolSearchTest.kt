package mycelium

import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.toolSearchText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class ToolSearchTest {
    private fun search(query: String?, status: ToolStatus = ToolStatus.Done, title: String = "ToolSearch") = ThreadItem.ToolCall(
        key = "k", callId = "t1", title = title, kind = "other", argument = title, status = status, diff = null, output = "Tool loaded.",
        input = buildJsonObject { query?.let { put("query", JsonPrimitive(it)) } },
    )

    @Test
    fun loadingNamedToolsSaysWhichToolsInPlainWords() {
        assertEquals("Loaded Remember from Mycelium", toolSearchText(search("select:mcp__mycelium__remember")))
        assertEquals("Loaded WebFetch, read file from Notes", toolSearchText(search("select:WebFetch,mcp__notes__read_file")))
        assertEquals("Loading Remember from Mycelium", toolSearchText(search("select:mcp__mycelium__remember", ToolStatus.Running)))
    }

    @Test
    fun aKeywordSearchQuotesWhatWasSearchedFor() {
        assertEquals("Searched tools for “notebook jupyter”", toolSearchText(search("notebook jupyter")))
    }

    @Test
    fun otherToolsAreLeftAlone() {
        assertNull(toolSearchText(search("select:x", title = "Read")))
    }
}
