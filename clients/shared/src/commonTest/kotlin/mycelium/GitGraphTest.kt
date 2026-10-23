package mycelium

import mycelium.api.GitBranch
import mycelium.api.GitBranches
import mycelium.api.GitRef
import mycelium.api.GraphCommit
import mycelium.state.BranchEntry
import mycelium.state.GraphEdge
import mycelium.state.branchEntries
import mycelium.state.graphDescription
import mycelium.state.graphRows
import mycelium.state.headLane
import mycelium.state.splitCommitDiff
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

private fun commit(hash: String, vararg parents: String, refs: List<GitRef> = emptyList()) =
    GraphCommit(hash, parents.toList(), "Sam", "2026-01-01T00:00:00Z", refs, "Commit $hash")

private fun branch(name: String) = GitBranch(name, "Sam", "2026-01-01T00:00:00Z", "Subject of $name")

class GitGraphTest {
    @Test
    fun aLinearHistoryStaysInOneColumn() {
        val rows = graphRows(listOf(commit("c", "b"), commit("b", "a"), commit("a")))

        assertEquals(listOf(0, 0, 0), rows.map { it.column })
        assertEquals(listOf(1, 1, 1), rows.map { it.width })
        assertEquals(listOf(GraphEdge(0, 0, rows[0].lane)), rows[0].below)
        assertTrue(rows[2].below.isEmpty())
    }

    @Test
    fun aMergeOpensASecondLaneThatTheCommonAncestorClosesAgain() {
        val rows = graphRows(listOf(commit("m", "b", "f"), commit("b", "a"), commit("f", "a"), commit("a")))
        val (merge, main, side, base) = rows

        assertTrue(merge.isMerge)
        assertEquals(listOf(0, 1), merge.below.map { it.to })
        assertEquals(0, main.column)
        assertEquals(1, side.column)
        assertEquals(listOf(GraphEdge(0, 0, main.lane)), side.through)
        assertEquals(listOf(GraphEdge(1, 0, main.lane)), side.below)
        assertEquals(listOf(0), base.above.map { it.from })
        assertEquals(0, base.column)
        assertEquals(2, side.width)
    }

    @Test
    fun anOctopusMergeDrawsOneLanePerParent() {
        val rows = graphRows(listOf(commit("m", "a", "b", "c"), commit("a"), commit("b"), commit("c")))

        assertEquals(listOf(0, 1, 2), rows[0].below.map { it.to })
        assertEquals(3, rows[0].width)
        assertEquals(listOf(0, 1, 2), rows.drop(1).map { it.column })
        assertEquals(setOf(0, 1, 2), rows[0].below.map { it.lane }.toSet())
    }

    @Test
    fun manyBranchesSharingOneParentJoinBackIntoOneColumn() {
        val rows = graphRows(listOf(commit("x", "r"), commit("y", "r"), commit("z", "r"), commit("r")))

        assertEquals(listOf(0, 1, 1, 0), rows.map { it.column })
        assertEquals(listOf(0, 0), rows.subList(1, 3).flatMap { row -> row.below.map { it.to } })
    }

    @Test
    fun aFreedColumnIsReusedByTheNextBranch() {
        val rows = graphRows(listOf(commit("m", "a", "b"), commit("b"), commit("n", "c"), commit("a", "c"), commit("c")))

        assertEquals(1, rows[1].column)
        assertEquals(1, rows[2].column)
    }

    @Test
    fun theHeadLaneIsTheLaneOfTheCommitWithTheCurrentBranch() {
        val rows = graphRows(listOf(commit("f", "a"), commit("m", "a", refs = listOf(GitRef("main", "branch", current = true))), commit("a")))

        assertEquals(rows[1].lane, headLane(rows))
        assertNull(headLane(graphRows(listOf(commit("a")))))
    }

    @Test
    fun aRowDescriptionNamesSubjectAuthorTimeAndEveryRefInWords() {
        val tip = commit("a", refs = listOf(GitRef("main", "branch", true), GitRef("origin/main", "remote"), GitRef("v1", "tag")))

        assertEquals(
            "Commit a, Sam, 5m ago, current branch main, remote branch origin/main, tag v1",
            graphDescription(tip, kotlin.time.Instant.parse("2026-01-01T00:05:00Z")),
        )
    }

    @Test
    fun aCommitPatchSplitsIntoOneDiffPerFile() {
        val files = splitCommitDiff("diff --git a/x.kt b/x.kt\n--- a/x.kt\n+++ b/x.kt\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/dir/y.kt b/dir/y.kt\n@@ -0,0 +1 @@\n+added\n")

        assertEquals(listOf("x.kt", "dir/y.kt"), files.map { it.path })
        assertEquals(listOf(1, 1), files.map { it.added })
        assertEquals(1, files[0].removed)
    }

    @Test
    fun theBranchSearchHidesRemotesThatAlreadyHaveALocalBranchAndOffersCreationForANewName() {
        val branches = GitBranches("main", listOf(branch("main"), branch("topic")), listOf(branch("origin/main"), branch("origin/other")))

        val all = branchEntries(branches, "")

        assertEquals(listOf("main", "topic"), all.filterIsInstance<BranchEntry.Local>().map { it.branch.name })
        assertEquals(listOf("other"), all.filterIsInstance<BranchEntry.Remote>().map { it.localName })
        assertTrue(all.none { it is BranchEntry.Create })
        assertEquals(listOf(BranchEntry.Create("fresh")), branchEntries(branches, "fresh"))
        assertTrue(branchEntries(branches, "topic").none { it is BranchEntry.Create })
        assertTrue(branchEntries(branches, "origin/other").none { it is BranchEntry.Create })
        assertTrue(branchEntries(branches, "main").filterIsInstance<BranchEntry.Local>().single().current)
    }
}
