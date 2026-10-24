package mycelium.state

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import mycelium.Platform
import mycelium.api.ApiException
import mycelium.api.Artifact
import mycelium.api.PatchArtifactRequest
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.CreateProjectRequest
import mycelium.api.DeclineInstructionProposalRequest
import mycelium.api.InstructionProposal
import mycelium.api.ConfirmScheduleRequest
import mycelium.api.CreateScheduleRequest
import mycelium.api.CreateTaskRequest
import mycelium.api.ApiAccessRequest
import mycelium.api.SshAccessRequest
import mycelium.api.MessageProposal
import mycelium.api.MoveProgress
import mycelium.api.MoveRequest
import mycelium.api.MoveState
import mycelium.api.PatchScheduleRequest
import mycelium.api.Schedule
import mycelium.api.SetGoalRequest
import mycelium.api.ScheduleRun
import mycelium.api.Task
import mycelium.api.TaskStatuses
import mycelium.api.Project
import mycelium.api.MyceliumApi
import mycelium.api.StreamMessage
import mycelium.api.WalkthroughNote
import mycelium.api.WalkthroughNoteRequest
import mycelium.api.NodeResources
import mycelium.api.EventKinds
import kotlinx.serialization.json.JsonObject
import mycelium.api.PatchConversationRequest
import kotlin.io.encoding.Base64

private const val EventPageSize = 500
private const val FirstRetryMillis = 1_000L
private const val LongestRetryMillis = 15_000L
private const val MaxCachedAttachments = 20
private const val MaxCachedPreviews = 200

class Store(val api: MyceliumApi, private val scope: CoroutineScope, private val platform: Platform? = null) {
    private val mutableState = MutableStateFlow(AppState())
    private val mutableProblem = MutableStateFlow<String?>(null)
    private var streaming: Job? = null
    private val typed = Channel<Pair<String, String>>(Channel.UNLIMITED)
    private val attachmentCache = mutableMapOf<String, ByteArray>()
    private val previewCache = LinkedHashMap<String, ByteArray>()

    val composerDrafts = ComposerDrafts()
    val pinDismissals = PinDismissals()

    var viewing: String? = null

    private val mutableOpenRequests = MutableSharedFlow<String>(extraBufferCapacity = 1)
    val openRequests: SharedFlow<String> = mutableOpenRequests.asSharedFlow()

    val state: StateFlow<AppState> = mutableState.asStateFlow()
    val problem: StateFlow<String?> = mutableProblem.asStateFlow()
    private val mutableWalkthroughScopes = MutableStateFlow<Map<String, WalkthroughScope>>(emptyMap())
    val walkthroughScopes: StateFlow<Map<String, WalkthroughScope>> = mutableWalkthroughScopes.asStateFlow()
    private val mutableDiffTabs = MutableStateFlow<Map<String, DiffTabs>>(emptyMap())
    val diffTabs: StateFlow<Map<String, DiffTabs>> = mutableDiffTabs.asStateFlow()
    private val mutableChangesFile = MutableStateFlow<Map<String, String>>(emptyMap())
    val changesFile: StateFlow<Map<String, String>> = mutableChangesFile.asStateFlow()
    private val mutableWalkthroughNotes = MutableStateFlow<Map<String, List<WalkthroughNote>>>(emptyMap())
    val walkthroughNotes: StateFlow<Map<String, List<WalkthroughNote>>> = mutableWalkthroughNotes.asStateFlow()

    init {
        scope.launch {
            var carried: Pair<String, String>? = null

            while (true) {
                val (terminalId, first) = carried ?: typed.receive()
                var text = first
                carried = null

                while (true) {
                    val next = typed.tryReceive().getOrNull() ?: break

                    if (next.first != terminalId) {
                        carried = next
                        break
                    }

                    text += next.second
                }

                guarded { api.terminalInput(terminalId, Base64.encode(text.encodeToByteArray())) }
            }
        }
    }

    fun start() {
        if (streaming != null) {
            return
        }

        streaming = scope.launch {
            var retry = FirstRetryMillis

            while (true) {
                try {
                    refresh()
                    state.value.openedConversations.forEach { open(it) }
                    mutableState.update { it.copy(connected = true) }
                    retry = FirstRetryMillis
                    api.stream().collect { message -> receive(message) }
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (_: Exception) {
                    mutableState.update { it.copy(connected = false) }
                }

                mutableState.update { it.copy(connected = false) }
                delay(retry)
                retry = minOf(retry * 2, LongestRetryMillis)
            }
        }
    }

    fun receive(message: StreamMessage) {
        val before = state.value
        mutableState.update { reduce(it, message) }

        if (message is StreamMessage.WalkthroughNotesChanged) {
            mutableWalkthroughNotes.update { it + (message.conversationId to message.notes) }
        }

        announceFinished(before, message)
    }

    fun openFromNotification(conversationId: String) {
        mutableOpenRequests.tryEmit(conversationId)
    }

    private fun announceFinished(before: AppState, message: StreamMessage) {
        val platform = platform ?: return

        if (message !is StreamMessage.ConversationChanged) {
            return
        }

        val updated = message.conversation
        val previous = before.conversations[updated.id] ?: return
        val outcome = finishedWording(updated.status)

        val queued = !before.queues[updated.id].isNullOrEmpty()

        if (previous.status != ConversationStatus.Working || outcome == null || updated.sideKind != null || queued) {
            return
        }

        if (updated.id == viewing && platform.windowFocused()) {
            return
        }

        val node = before.nodes[updated.nodeId]?.name
        val wantsSound = platform.loadPreference(NotifySoundPreference) != false.toString()

        if (platform.loadPreference(NotifyFinishedPreference) != false.toString()) {
            platform.notify(updated.title, listOfNotNull(outcome, node).joinToString(" · "), updated.id, wantsSound)
        }

        if (wantsSound) {
            platform.chime()
        }
    }

    fun stop() {
        streaming?.cancel()
        streaming = null
    }

    fun reconnect() {
        if (streaming == null) {
            return
        }

        stop()
        start()
    }

    suspend fun refresh() {
        val conversations = api.conversations(archived = false)
        val nodes = api.nodes()

        mutableState.update { it.copy(conversations = conversations.associateBy { c -> c.id }, nodes = nodes.associateBy { n -> n.id }) }
    }

    suspend fun watchNode(id: String) {
        val watched = try {
            api.watchNode(id)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            return
        }

        mutableState.update {
            val held = it.resources[id]

            it.copy(resources = it.resources + (id to NodeResources(watched.latest ?: held?.latest, watched.history.ifEmpty { held?.history.orEmpty() })))
        }
    }

    suspend fun refreshProjects() {
        val projects = api.projects()

        mutableState.update { it.copy(projects = projects.associateBy { p -> p.id }) }
    }

    suspend fun refreshGroups() {
        val groups = api.groups()

        mutableState.update { it.copy(groups = groups.associateBy { group -> group.id }) }
    }

    suspend fun createProject(request: CreateProjectRequest): Project {
        val project = api.createProject(request)

        mutableState.update { it.copy(projects = it.projects + (project.id to project)) }

        return project
    }

    suspend fun requestMove(conversationId: String, request: MoveRequest): MoveProgress {
        val progress = api.requestMove(conversationId, request)

        mutableState.update { it.copy(moves = it.moves + (conversationId to progress)) }

        return progress
    }

    suspend fun cancelMove(conversationId: String) {
        api.cancelMove(conversationId)

        mutableState.update { held -> held.moves[conversationId]?.let { held.copy(moves = held.moves + (conversationId to it.copy(state = MoveState.Cancelled))) } ?: held }
    }

    suspend fun setGoal(conversationId: String, text: String, limits: GoalLimits) {
        val goal = api.setGoal(conversationId, SetGoalRequest(text, limits.tokenBudget, limits.timeLimitSeconds, limits.fiveHourPercent, limits.weeklyPercent))

        mutableState.update { it.copy(goals = it.goals + (conversationId to goal)) }
    }

    suspend fun pauseGoal(conversationId: String, paused: Boolean) {
        val goal = api.pauseGoal(conversationId, paused)

        mutableState.update { it.copy(goals = it.goals + (conversationId to goal)) }
    }

    suspend fun clearGoal(conversationId: String) {
        api.clearGoal(conversationId)

        mutableState.update { it.copy(goals = it.goals - conversationId) }
    }

    suspend fun setProjectGroup(projectId: String, groupId: String?): Project = keepProject(api.setProjectGroup(projectId, groupId))

    suspend fun setProjectIcon(projectId: String, icon: String?): Project = keepProject(api.setProjectIcon(projectId, icon))

    suspend fun uploadProjectAvatar(projectId: String, mediaType: String, bytes: ByteArray): Project = keepProject(api.uploadProjectAvatar(projectId, mediaType, bytes))

    private fun keepProject(project: Project): Project {
        mutableState.update { it.copy(projects = it.projects + (project.id to project)) }

        return project
    }

    suspend fun refreshBoard() {
        val projects = api.projects()
        val worktrees = api.worktrees()
        val tasks = api.tasks()

        mutableState.update { it.copy(projects = projects.associateBy { p -> p.id }, worktrees = worktrees.associateBy { w -> w.id }, tasks = tasks.associateBy { t -> t.id }) }
    }

    suspend fun createTask(request: CreateTaskRequest): Task {
        val task = api.createTask(request)

        mutableState.update { it.copy(tasks = it.tasks + (task.id to task)) }

        return task
    }

    suspend fun startTask(task: Task, conversationId: String? = null): Conversation {
        val conversation = api.startTask(task.id, conversationId)

        mutableState.update { it.copy(tasks = it.tasks + (task.id to task.copy(status = TaskStatuses.Claimed, conversationId = conversation.id)), conversations = it.conversations + (conversation.id to conversation)) }

        return conversation
    }

    suspend fun approveTask(task: Task) = replaceTask(api.approveTask(task.id))

    suspend fun declineTask(task: Task) = replaceTask(api.declineTask(task.id))

    suspend fun answerTask(task: Task, answer: String) = replaceTask(api.answerTask(task.id, answer))

    suspend fun finishTask(task: Task) = replaceTask(api.finishTask(task.id))

    private fun replaceTask(task: Task) {
        mutableState.update { it.copy(tasks = it.tasks + (task.id to task)) }
    }

    suspend fun deleteTask(task: Task) {
        api.deleteTask(task.id)

        mutableState.update { it.copy(tasks = it.tasks - task.id) }
    }

    suspend fun refreshInstructionProposals() {
        val proposals = api.instructionProposals()

        mutableState.update { it.copy(instructionProposals = proposals.associateBy { proposal -> proposal.id }) }
    }

    suspend fun approveInstructionProposal(proposal: InstructionProposal) {
        val approved = api.approveInstructionProposal(proposal.id)

        mutableState.update { it.copy(instructionProposals = it.instructionProposals + (approved.id to approved)) }
    }

    suspend fun declineInstructionProposal(proposal: InstructionProposal, note: String?, redo: Boolean) {
        val declined = api.declineInstructionProposal(proposal.id, DeclineInstructionProposalRequest(note?.trim()?.takeIf { it.isNotEmpty() }, redo))

        mutableState.update { it.copy(instructionProposals = it.instructionProposals + (declined.id to declined)) }
    }

    suspend fun refreshMessageProposals() {
        val proposals = api.messageProposals()

        mutableState.update { it.copy(messageProposals = proposals.associateBy { proposal -> proposal.id }) }
    }

    suspend fun approveMessageProposal(proposal: MessageProposal) {
        val sent = api.approveMessageProposal(proposal.id)

        mutableState.update { it.copy(messageProposals = it.messageProposals + (sent.id to sent)) }
    }

    suspend fun declineMessageProposal(proposal: MessageProposal) {
        val declined = api.declineMessageProposal(proposal.id)

        mutableState.update { it.copy(messageProposals = it.messageProposals + (declined.id to declined)) }
    }

    suspend fun refreshApiAccessRequests() {
        val requests = api.apiAccessRequests()

        mutableState.update { it.copy(apiAccessRequests = requests.associateBy { request -> request.id }) }
    }

    suspend fun approveApiAccessRequest(request: ApiAccessRequest, scopes: List<String>) {
        val approved = api.approveApiAccessRequest(request.id, scopes)

        mutableState.update { it.copy(apiAccessRequests = it.apiAccessRequests + (approved.id to approved)) }
    }

    suspend fun declineApiAccessRequest(request: ApiAccessRequest) {
        val declined = api.declineApiAccessRequest(request.id)

        mutableState.update { it.copy(apiAccessRequests = it.apiAccessRequests + (declined.id to declined)) }
    }

    suspend fun refreshSshAccessRequests() {
        val requests = api.sshAccessRequests()

        mutableState.update { it.copy(sshAccessRequests = requests.associateBy { request -> request.id }) }
    }

    suspend fun approveSshAccessRequest(request: SshAccessRequest, minutes: Int) {
        val approved = api.approveSshAccessRequest(request.id, minutes)

        mutableState.update { it.copy(sshAccessRequests = it.sshAccessRequests + (approved.id to approved)) }
    }

    suspend fun declineSshAccessRequest(request: SshAccessRequest) {
        val declined = api.declineSshAccessRequest(request.id)

        mutableState.update { it.copy(sshAccessRequests = it.sshAccessRequests + (declined.id to declined)) }
    }

    suspend fun revokeSshAccessRequest(request: SshAccessRequest) {
        val revoked = api.revokeSshAccessRequest(request.id)

        mutableState.update { it.copy(sshAccessRequests = it.sshAccessRequests + (revoked.id to revoked)) }
    }

    suspend fun refreshArtifacts() {
        val artifacts = api.artifacts()

        mutableState.update { it.copy(artifacts = artifacts.associateBy { artifact -> artifact.id }) }
    }

    suspend fun renameArtifact(artifact: Artifact, title: String) {
        val changed = api.editArtifact(artifact.id, PatchArtifactRequest(title = title))

        mutableState.update { it.copy(artifacts = it.artifacts + (changed.id to changed)) }
    }

    suspend fun setArtifactPublic(artifact: Artifact, isPublic: Boolean) {
        val changed = api.editArtifact(artifact.id, PatchArtifactRequest(isPublic = isPublic))

        mutableState.update { it.copy(artifacts = it.artifacts + (changed.id to changed)) }
    }

    suspend fun deleteArtifact(artifact: Artifact) {
        api.deleteArtifact(artifact.id)

        mutableState.update { it.copy(artifacts = it.artifacts - artifact.id) }
    }

    suspend fun refreshSchedules() {
        val schedules = api.schedules()
        val runs = api.scheduleRuns()

        mutableState.update { it.copy(schedules = schedules.associateBy { s -> s.id }, scheduleRuns = runs) }
    }

    suspend fun createSchedule(request: CreateScheduleRequest): Schedule {
        val schedule = api.createSchedule(request)

        mutableState.update { it.copy(schedules = it.schedules + (schedule.id to schedule)) }

        return schedule
    }

    suspend fun pauseSchedule(schedule: Schedule, paused: Boolean) {
        val changed = api.pauseSchedule(schedule.id, paused)

        mutableState.update { it.copy(schedules = it.schedules + (changed.id to changed)) }
    }

    suspend fun editSchedule(schedule: Schedule, request: PatchScheduleRequest) {
        val changed = api.editSchedule(schedule.id, request)

        mutableState.update { it.copy(schedules = it.schedules + (changed.id to changed)) }
    }

    suspend fun runSchedule(schedule: Schedule): ScheduleRun {
        val run = api.runSchedule(schedule.id)

        refreshSchedules()

        return run
    }

    suspend fun confirmSchedule(schedule: Schedule, request: ConfirmScheduleRequest) {
        val confirmed = api.confirmSchedule(schedule.id, request)

        mutableState.update { it.copy(schedules = it.schedules + (confirmed.id to confirmed)) }
    }

    suspend fun declineSchedule(schedule: Schedule) {
        api.declineSchedule(schedule.id)

        mutableState.update { it.copy(schedules = it.schedules - schedule.id) }
    }

    suspend fun setTimeZone(timeZone: String) {
        api.setTimeZone(timeZone)
    }

    suspend fun deleteSchedule(schedule: Schedule) {
        api.deleteSchedule(schedule.id)

        mutableState.update { it.copy(schedules = it.schedules - schedule.id, scheduleRuns = it.scheduleRuns.filter { run -> run.scheduleId != schedule.id }) }
    }

    suspend fun loadOlder(conversationId: String) {
        val oldest = state.value.events[conversationId]?.firstOrNull()?.seq ?: return
        var claimed = false

        mutableState.update {
            claimed = conversationId !in it.loadingOlderEvents && it.hasOlderEvents(conversationId)

            if (claimed) it.copy(loadingOlderEvents = it.loadingOlderEvents + conversationId) else it
        }

        if (!claimed) {
            return
        }

        try {
            val page = api.events(conversationId, before = oldest, limit = EventPageSize)

            mutableState.update {
                it.copy(
                    events = it.events + (conversationId to mergeEvents(it.events[conversationId].orEmpty(), page)),
                    completeHistories = if (page.isEmpty()) it.completeHistories + conversationId else it.completeHistories,
                )
            }
        } finally {
            mutableState.update { it.copy(loadingOlderEvents = it.loadingOlderEvents - conversationId) }
        }
    }

    suspend fun open(conversationId: String) {
        val reopened = conversationId in state.value.openedConversations
        var after = if (reopened) state.value.events[conversationId]?.lastOrNull()?.seq ?: 0 else 0
        mutableState.update { it.copy(openedConversations = it.openedConversations + conversationId) }

        val fetched = mutableListOf<mycelium.api.Event>()

        if (reopened) {
            while (true) {
                val page = api.events(conversationId, after, EventPageSize)
                fetched += page

                if (page.size < EventPageSize) {
                    break
                }

                after = page.maxOf { it.seq }
            }
        } else {
            fetched += api.events(conversationId, before = Long.MAX_VALUE, limit = EventPageSize)
        }

        mutableState.update {
            val delivered = fetched.filter { event -> event.kind == EventKinds.UserMessage }.mapNotNull { event -> (event.payload as? JsonObject)?.string("text") }
            val sending = delivered.fold(it.sending) { waiting, text -> waiting.withoutFirst(conversationId, text) }

            it.copy(events = it.events + (conversationId to mergeEvents(it.events[conversationId].orEmpty(), fetched)), sending = sending)
        }

        val sessionUpdates = orNull { api.sessionUpdates(conversationId) }.orEmpty()
        mutableState.update { it.copy(sessionUpdates = it.sessionUpdates + (conversationId to sessionUpdates)) }

        val queue = api.queue(conversationId)
        mutableState.update { it.copy(queues = it.queues + (conversationId to queue.sortedBy { q -> q.position })) }

        val goal = orNull { api.goal(conversationId) }
        mutableState.update { it.copy(goals = if (goal == null) it.goals - conversationId else it.goals + (conversationId to goal)) }

        val walkthrough = orNull { api.walkthrough(conversationId) }
        mutableState.update { it.copy(walkthroughs = if (walkthrough == null) it.walkthroughs - conversationId else it.walkthroughs + (conversationId to walkthrough)) }

        walkthrough?.let { loadWalkthroughNotes(conversationId, it.id) }

        val conversation = state.value.conversations[conversationId]

        if (conversation != null && conversation.status == mycelium.api.ConversationStatus.Review) {
            api.markOpened(conversationId)
        }
    }

    fun send(conversationId: String, text: String, files: List<PickedFile> = emptyList()) {
        val prompt = text.trim()
        val shownWhileSending = prompt.isNotEmpty()

        if (shownWhileSending) {
            mutableState.update { it.copy(sending = it.sending + (conversationId to it.sending[conversationId].orEmpty() + prompt)) }
        }

        act {
            try {
                val attachments = files.map { file -> api.uploadAttachment(conversationId, file.name, file.mediaType, file.bytes) }
                api.sendPrompt(conversationId, prompt, attachments)
            } catch (failure: Exception) {
                if (shownWhileSending) {
                    mutableState.update { it.copy(sending = it.sending.withoutFirst(conversationId, prompt)) }
                }

                throw failure
            }
        }
    }

    fun chooseWalkthroughScope(conversationId: String, walkthroughScope: WalkthroughScope) {
        mutableWalkthroughScopes.update { it + (conversationId to walkthroughScope) }
    }

    fun regenerateWalkthrough(conversationId: String) {
        send(conversationId, regenerateWalkthroughPrompt(mutableWalkthroughScopes.value[conversationId] ?: WalkthroughScope.Branch))
    }

    fun addWalkthroughNote(conversationId: String, walkthroughId: String, request: WalkthroughNoteRequest) {
        act {
            val note = api.addWalkthroughNote(walkthroughId, request)
            mutableWalkthroughNotes.update { it + (conversationId to it[conversationId].orEmpty() + note) }
        }
    }

    suspend fun loadWalkthroughNotes(conversationId: String, walkthroughId: String) {
        val notes = orNull { api.walkthroughNotes(walkthroughId) } ?: return
        mutableWalkthroughNotes.update { it + (conversationId to notes) }
    }

    fun withWalkthroughNotes(conversationId: String, message: String): String {
        val walkthrough = state.value.walkthroughs[conversationId] ?: return message
        val block = walkthroughNotesBlock(walkthrough, mutableWalkthroughNotes.value[conversationId].orEmpty()) ?: return message

        mutableWalkthroughNotes.update { it + (conversationId to it[conversationId].orEmpty().map { note -> note.copy(sent = true) }) }
        act { api.markWalkthroughNotesSent(walkthrough.id) }

        return listOf(block, message.trim()).filter { it.isNotEmpty() }.joinToString("\n\n")
    }

    fun removeWalkthroughNote(conversationId: String, walkthroughId: String, note: WalkthroughNote) {
        mutableWalkthroughNotes.update { it + (conversationId to it[conversationId].orEmpty() - note) }
        act { api.removeWalkthroughNote(walkthroughId, note.id) }
    }

    fun openDiffTab(conversationId: String, tab: DiffTab) {
        mutableDiffTabs.update { it + (conversationId to (it[conversationId] ?: DiffTabs()).opened(tab)) }
    }

    fun focusDiffTab(conversationId: String, key: String?) {
        mutableDiffTabs.update { it + (conversationId to (it[conversationId] ?: DiffTabs()).copy(active = key)) }
    }

    fun revealedInDiffTab(conversationId: String, key: String) {
        mutableDiffTabs.update { it + (conversationId to (it[conversationId] ?: DiffTabs()).revealed(key)) }
    }

    fun viewingChangesFile(conversationId: String, path: String?) {
        mutableChangesFile.update { if (path == null) it - conversationId else it + (conversationId to path) }
    }

    fun closeDiffTab(conversationId: String, key: String) {
        mutableDiffTabs.update { it + (conversationId to (it[conversationId] ?: DiffTabs()).closed(key)) }
    }

    fun setWalkthroughDismissed(conversationId: String, dismissed: Boolean) {
        val walkthrough = state.value.walkthroughs[conversationId] ?: return
        mutableState.update { it.copy(walkthroughs = it.walkthroughs + (conversationId to walkthrough.copy(dismissed = dismissed))) }
        act { if (dismissed) api.dismissWalkthrough(walkthrough.id) else api.reopenWalkthrough(walkthrough.id) }
    }

    suspend fun attachmentBytes(sha256: String): ByteArray? {
        attachmentCache[sha256]?.let { return it }

        val bytes = runCatching { api.attachmentBytes(sha256) }.getOrNull() ?: return null

        if (attachmentCache.size >= MaxCachedAttachments) {
            attachmentCache.clear()
        }

        attachmentCache[sha256] = bytes

        return bytes
    }

    suspend fun artifactThumbnail(artifact: Artifact): ByteArray? {
        val version = artifact.thumbnailVersion ?: return null

        return cachedPreview("thumbnail:${artifact.id}:$version") { api.artifactThumbnail(artifact.id) }
    }

    suspend fun artifactContent(artifact: Artifact): ByteArray? = cachedPreview("content:${artifact.id}:${artifact.latestVersion}") { api.artifactContent(artifact.id, artifact.latestVersion) }

    private suspend fun cachedPreview(key: String, fetch: suspend () -> ByteArray): ByteArray? {
        previewCache[key]?.let { return it }

        val bytes = runCatching { fetch() }.getOrNull() ?: return null

        if (previewCache.size >= MaxCachedPreviews) {
            previewCache.remove(previewCache.keys.first())
        }

        previewCache[key] = bytes

        return bytes
    }

    fun typeIntoTerminal(terminalId: String, text: String) {
        typed.trySend(terminalId to text)
    }

    fun stopTerminal(terminalId: String) {
        val exited = terminalId in state.value.exitedTerminals
        mutableState.update { it.copy(terminalOutput = it.terminalOutput - terminalId, exitedTerminals = it.exitedTerminals - terminalId) }

        if (!exited) {
            act { api.stopTerminal(terminalId) }
        }
    }

    fun rename(conversation: Conversation, title: String) {
        if (title.isNotEmpty()) {
            mutableState.update { it.copy(conversations = it.conversations + (conversation.id to conversation.copy(title = title, titleUserSet = true))) }
        }

        act {
            try {
                val request = if (title.isEmpty()) PatchConversationRequest(clearTitle = true) else PatchConversationRequest(title = title)
                val renamed = api.patchConversation(conversation.id, request)
                mutableState.update { it.copy(conversations = it.conversations + (renamed.id to renamed)) }
            } catch (failure: Exception) {
                mutableState.update { it.copy(conversations = it.conversations + (conversation.id to conversation)) }
                throw failure
            }
        }
    }

    fun pin(conversation: Conversation, pinned: Boolean) {
        act {
            val updated = api.patchConversation(conversation.id, PatchConversationRequest(pinned = pinned))
            mutableState.update { it.copy(conversations = it.conversations + (updated.id to updated)) }
        }
    }

    fun closeSession(conversation: Conversation) {
        act { api.closeSession(conversation.id) }
    }

    fun act(block: suspend () -> Unit) {
        scope.launch {
            mutableProblem.value = null
            guarded(block)
        }
    }

    private suspend fun guarded(block: suspend () -> Unit) {
        try {
            block()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: ApiException) {
            mutableProblem.value = failure.message
        } catch (_: Exception) {
            mutableProblem.value = "Could not reach the control plane. Check your connection and try again."
        }
    }

    private suspend fun <T> orNull(block: suspend () -> T?): T? =
        try {
            block()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            null
        }

    fun dismissProblem() {
        mutableProblem.value = null
    }
}
