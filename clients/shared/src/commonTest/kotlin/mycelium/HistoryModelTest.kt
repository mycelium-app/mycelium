package mycelium

import mycelium.api.ActivityRow
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.ui.HistoryColumn
import mycelium.ui.HistoryEntry
import mycelium.ui.HistoryFilters
import mycelium.ui.HistoryGrouping
import mycelium.ui.HistoryLayout
import mycelium.ui.HistorySort
import mycelium.ui.HistoryView
import mycelium.ui.SortDirection
import mycelium.ui.after
import mycelium.ui.decodeHistoryColumns
import mycelium.ui.decodeHistoryViews
import mycelium.ui.decodeHistoryWidths
import mycelium.ui.deleteHistoryView
import mycelium.ui.encodeHistoryColumns
import mycelium.ui.encodeHistoryViews
import mycelium.ui.encodeHistoryWidths
import mycelium.ui.filterHistory
import mycelium.ui.groupHistory
import mycelium.ui.historyOptions
import mycelium.ui.historyViewNameProblem
import mycelium.ui.renameHistoryView
import mycelium.ui.saveHistoryView
import mycelium.ui.sortHistory
import mycelium.ui.standardHistoryColumns
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull

class HistoryModelTest {
    private fun entry(
        id: String,
        project: String = "okapi",
        node: String = "laptop",
        updatedAt: String = "2026-10-22T10:00:00Z",
        tokens: Long? = null,
        agent: String = "claude",
        archived: Boolean = false,
    ) = HistoryEntry(
        Conversation(
            id = id, owner = "u", nodeId = "n", agent = agent, cwd = "/work", title = id, status = ConversationStatus.Done,
            createdAt = "2026-10-20T10:00:00Z", updatedAt = updatedAt, archivedAt = if (archived) "2026-10-23T10:00:00Z" else null,
        ),
        project,
        node,
        tokens?.let { ActivityRow(key = id, name = "", turns = 1, reportedTurns = 1, inputTokens = it) },
    )

    private fun ids(rows: List<HistoryEntry>) = rows.map { it.conversation.id }

    @Test
    fun clickingAColumnSortsAscendingThenDescending() {
        val first = HistorySort(HistoryColumn.LastActive, SortDirection.Descending).after(HistoryColumn.Title)
        val second = first.after(HistoryColumn.Title)

        assertEquals(HistorySort(HistoryColumn.Title, SortDirection.Ascending), first)
        assertEquals(HistorySort(HistoryColumn.Title, SortDirection.Descending), second)
        assertEquals(HistorySort(HistoryColumn.Title, SortDirection.Ascending), second.after(HistoryColumn.Title))
    }

    @Test
    fun sortingByTokensKeepsConversationsWithoutUsageLastInEitherDirection() {
        val rows = listOf(entry("none"), entry("small", tokens = 10), entry("large", tokens = 500))

        assertEquals(listOf("small", "large", "none"), ids(sortHistory(rows, HistorySort(HistoryColumn.Tokens, SortDirection.Ascending))))
        assertEquals(listOf("large", "small", "none"), ids(sortHistory(rows, HistorySort(HistoryColumn.Tokens, SortDirection.Descending))))
    }

    @Test
    fun sortingByTitleIgnoresCase() {
        val rows = listOf(entry("beta"), entry("Alpha"), entry("gamma"))

        assertEquals(listOf("Alpha", "beta", "gamma"), ids(sortHistory(rows, HistorySort(HistoryColumn.Title, SortDirection.Ascending))))
    }

    @Test
    fun sortingByLastActiveNewestFirstPutsTheLatestAtTheTop() {
        val rows = listOf(entry("old", updatedAt = "2026-10-01T10:00:00Z"), entry("new", updatedAt = "2026-10-22T10:00:00Z"))

        assertEquals(listOf("new", "old"), ids(sortHistory(rows, HistorySort(HistoryColumn.LastActive, SortDirection.Descending))))
    }

    @Test
    fun filtersNarrowByProjectNodeAndAgentTogether() {
        val rows = listOf(
            entry("a", project = "okapi", node = "laptop"),
            entry("b", project = "okapi", node = "server"),
            entry("c", project = "pangolin", node = "server"),
            entry("d", project = "okapi", node = "server", agent = "codex"),
        )

        assertEquals(listOf("b", "c", "d"), ids(filterHistory(rows, HistoryFilters(node = "server"))))
        assertEquals(listOf("b", "d"), ids(filterHistory(rows, HistoryFilters(project = "okapi", node = "server"))))
        assertEquals(listOf("d"), ids(filterHistory(rows, HistoryFilters(project = "okapi", node = "server", agent = "Codex"))))
        assertEquals(ids(rows), ids(filterHistory(rows, HistoryFilters())))
    }

    @Test
    fun optionsListEachValueOnceWithoutBlanks() {
        val rows = listOf(entry("a", project = "pangolin"), entry("b", project = "Okapi"), entry("c", project = "okapi"), entry("d", project = "", node = ""))
        val options = historyOptions(rows)

        assertEquals(listOf("okapi", "Okapi", "pangolin").sortedBy { it.lowercase() }.toSet(), options.projects.toSet())
        assertEquals(listOf("laptop"), options.nodes)
        assertEquals(listOf("Claude Code"), options.agents)
    }

    @Test
    fun groupingByProjectCollectsRowsAndPutsTheUnnamedGroupLast() {
        val rows = listOf(entry("a", project = "pangolin"), entry("b", project = ""), entry("c", project = "okapi"), entry("d", project = "pangolin"))
        val groups = groupHistory(rows, HistoryGrouping.Project)

        assertEquals(listOf("okapi", "pangolin", "No project"), groups.map { it.label })
        assertEquals(listOf("a", "d"), ids(groups[1].rows))
    }

    @Test
    fun groupingByNodeLabelsRowsWithoutOne() {
        val groups = groupHistory(listOf(entry("a", node = ""), entry("b", node = "server")), HistoryGrouping.Node)

        assertEquals(listOf("server", "No node"), groups.map { it.label })
    }

    @Test
    fun noGroupingKeepsOneGroupInOrder() {
        val rows = listOf(entry("b"), entry("a"))
        val groups = groupHistory(rows, HistoryGrouping.None)

        assertEquals(1, groups.size)
        assertEquals(listOf("b", "a"), ids(groups.single().rows))
    }

    @Test
    fun viewsSaveRenameAndDeleteByName() {
        val layout = HistoryLayout(grouping = HistoryGrouping.Node)
        val saved = saveHistoryView(emptyList(), "  By node ", layout)

        assertEquals(listOf("By node"), saved.map { it.name })

        val replaced = saveHistoryView(saved, "by node", HistoryLayout(grouping = HistoryGrouping.Project))

        assertEquals(1, replaced.size)
        assertEquals(HistoryGrouping.Project, replaced.single().layout.grouping)
        assertEquals(listOf("Weekly"), renameHistoryView(saved, "By node", " Weekly ").map { it.name })
        assertEquals(emptyList(), deleteHistoryView(saved, "By node"))
    }

    @Test
    fun aViewNameMustBeFilledInAndUnique() {
        val views = listOf(HistoryView("Weekly", HistoryLayout()))

        assertNotNull(historyViewNameProblem(views, "  "))
        assertNotNull(historyViewNameProblem(views, "weekly"))
        assertNull(historyViewNameProblem(views, "Weekly", renaming = "Weekly"))
        assertNull(historyViewNameProblem(views, "Monthly"))
    }

    @Test
    fun viewsSurviveARoundTripThroughPreferences() {
        val layout = HistoryLayout(
            columns = listOf(HistoryColumn.Title, HistoryColumn.Cost),
            sort = HistorySort(HistoryColumn.Cost, SortDirection.Ascending),
            filters = HistoryFilters(project = "okapi", node = "laptop"),
            grouping = HistoryGrouping.Project,
        )
        val views = listOf(HistoryView("Spend", layout))

        assertEquals(views, decodeHistoryViews(encodeHistoryViews(views)))
    }

    @Test
    fun unreadablePreferencesFallBackToDefaults() {
        assertEquals(emptyList(), decodeHistoryViews("not json"))
        assertEquals(emptyList(), decodeHistoryViews(null))
        assertEquals(standardHistoryColumns(), decodeHistoryColumns("not json"))
        assertEquals(standardHistoryColumns(), decodeHistoryColumns("[]"))
        assertEquals(emptyMap(), decodeHistoryWidths("not json"))
    }

    @Test
    fun columnsAndWidthsSurviveARoundTripThroughPreferences() {
        val columns = listOf(HistoryColumn.Title, HistoryColumn.Model)
        val widths = mapOf(HistoryColumn.Project to 180f)

        assertEquals(columns, decodeHistoryColumns(encodeHistoryColumns(columns)))
        assertEquals(widths, decodeHistoryWidths(encodeHistoryWidths(widths)))
    }
}
