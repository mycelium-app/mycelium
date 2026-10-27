package mycelium

import mycelium.api.ChatToolInfo
import mycelium.api.IntegrationPolicy
import mycelium.api.PresetAskAll
import mycelium.api.PresetAskAllEdit
import mycelium.api.PresetDontAsk
import mycelium.state.ApprovalColumn
import mycelium.state.GroupTick
import mycelium.state.askWord
import mycelium.state.editWord
import mycelium.state.groupFollowsPreset
import mycelium.state.groupTick
import mycelium.state.isCustomised
import mycelium.state.presetSummary
import mycelium.state.resetTool
import mycelium.state.resolveApproval
import mycelium.state.toggleGroup
import mycelium.state.toggleTool
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

private val search = ChatToolInfo("search", kind = "read")
private val fetch = ChatToolInfo("fetch", kind = "read")
private val create = ChatToolInfo("create_document", kind = "write")
private val update = ChatToolInfo("update_document", kind = "write")
private val reads = listOf(search, fetch)
private val writes = listOf(create, update)

class ApprovalsTest {
    @Test
    fun theDefaultPresetAsksBeforeWritesOnly() {
        val policy = IntegrationPolicy()

        assertFalse(resolveApproval(policy, ApprovalColumn.Ask, search).value)
        assertTrue(resolveApproval(policy, ApprovalColumn.Ask, create).value)
        assertFalse(resolveApproval(policy, ApprovalColumn.Edit, create).value)
    }

    @Test
    fun theEditPresetAsksAndEditsEverything() {
        val policy = IntegrationPolicy(preset = PresetAskAllEdit)

        assertTrue(resolveApproval(policy, ApprovalColumn.Ask, search).value)
        assertTrue(resolveApproval(policy, ApprovalColumn.Edit, search).value)
    }

    @Test
    fun aToolThatFollowsItsPresetIsNotOwnAndATickedGroupSetsEveryToolInIt() {
        val policy = toggleGroup(IntegrationPolicy(), ApprovalColumn.Ask, "read", reads)

        assertTrue(reads.all { resolveApproval(policy, ApprovalColumn.Ask, it).value })
        assertTrue(reads.none { resolveApproval(policy, ApprovalColumn.Ask, it).own })
        assertFalse(groupFollowsPreset(policy, ApprovalColumn.Ask, "read", reads))
        assertEquals(GroupTick.All, groupTick(policy, ApprovalColumn.Ask, reads))
    }

    @Test
    fun tickingAGroupBackToWhatThePresetSaysFollowsThePresetAgain() {
        val ticked = toggleGroup(IntegrationPolicy(), ApprovalColumn.Ask, "read", reads)
        val back = toggleGroup(ticked, ApprovalColumn.Ask, "read", reads)

        assertEquals(IntegrationPolicy(), back)
        assertTrue(groupFollowsPreset(back, ApprovalColumn.Ask, "read", reads))
    }

    @Test
    fun aToolSetOnItsOwnIsSolidAndResetFollowsItsGroupAgain() {
        val changed = toggleTool(IntegrationPolicy(), ApprovalColumn.Ask, create)

        assertFalse(resolveApproval(changed, ApprovalColumn.Ask, create).value)
        assertTrue(resolveApproval(changed, ApprovalColumn.Ask, create).own)
        assertTrue(resolveApproval(changed, ApprovalColumn.Ask, update).value)
        assertEquals(GroupTick.Some, groupTick(changed, ApprovalColumn.Ask, writes))
        assertEquals(IntegrationPolicy(), resetTool(changed, create))
    }

    @Test
    fun changingATickBackToItsGroupDropsTheToolRule() {
        val changed = toggleTool(IntegrationPolicy(), ApprovalColumn.Ask, create)

        assertEquals(IntegrationPolicy(), toggleTool(changed, ApprovalColumn.Ask, create))
    }

    @Test
    fun aToolThatStopsAskingUnderAnAskingPresetReadsAlwaysApproved() {
        val changed = toggleTool(IntegrationPolicy(), ApprovalColumn.Ask, create)

        assertEquals("Always approved", askWord(changed, create))
        assertEquals("Yes", askWord(changed, update))
        assertEquals("No", askWord(changed, search))
        assertEquals("No", editWord(changed, create))
    }

    @Test
    fun askingAndEditingAreSetIndependently() {
        val policy = toggleTool(IntegrationPolicy(preset = PresetDontAsk), ApprovalColumn.Edit, search)

        assertFalse(resolveApproval(policy, ApprovalColumn.Ask, search).value)
        assertTrue(resolveApproval(policy, ApprovalColumn.Edit, search).value)
    }

    @Test
    fun theSummaryNamesThePresetAndSaysWhenItIsCustomised() {
        assertEquals("Ask before writes", presetSummary(IntegrationPolicy()))
        assertEquals("Ask before everything, customised", presetSummary(toggleTool(IntegrationPolicy(preset = PresetAskAll), ApprovalColumn.Ask, search)))
        assertFalse(isCustomised(IntegrationPolicy()))
    }
}
