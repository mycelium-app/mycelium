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
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import mycelium.DefaultEventPageSize
import mycelium.Platform
import mycelium.api.Directories
import mycelium.api.Experiments
import mycelium.api.ApiException
import mycelium.api.ControlPlaneUnreachable
import mycelium.api.Artifact
import mycelium.api.ArtifactAccessRequest
import mycelium.api.CreateArtifactRequest
import mycelium.api.PatchArtifactRequest
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.CreateGroupConnectionRequest
import mycelium.api.CreateProjectRequest
import mycelium.api.DeclineInstructionProposalRequest
import mycelium.api.InstructionProposal
import mycelium.api.ConfirmScheduleRequest
import mycelium.api.ContinueRequest
import mycelium.api.CreateScheduleRequest
import mycelium.api.CreateTaskRequest
import mycelium.api.ApiAccessRequest
import mycelium.api.SshAccessRequest
import mycelium.api.MessageProposal
import mycelium.api.StartEnvironmentRequest
import mycelium.api.StartProposal
import mycelium.api.StartProposalChoice
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
import kotlin.time.Clock
import kotlin.time.Instant
import mycelium.api.Hours
import kotlin.io.encoding.Base64

private const val FirstRetryMillis = 1_000L
private const val LongestRetryMillis = 15_000L
private const val MaxCachedAttachments = 20

private const val MaxCachedAvatars = 200
private const val MaxCachedPreviews = 200

class Store(val api: MyceliumApi, private val scope: CoroutineScope, private val platform: Platform? = null) {
    private val eventPageSize = platform?.eventPageSize ?: DefaultEventPageSize
    private val mutableState = MutableStateFlow(AppState())
    private val mutableProblem = MutableStateFlow<String?>(null)
    private var streaming: Job? = null
    private val typed = Channel<Pair<String, String>>(Channel.UNLIMITED)
    private val attachmentCache = mutableMapOf<String, ByteArray>()
    private val previewCache = LinkedHashMap<String, ByteArray>()
    private val experimentsLock = Mutex()

    val wellness = WellnessStore(api, scope, platform)
    val voice = VoiceStore(api, scope, platform)
    val learning = LearningStore(api, scope)
    val composerDrafts = ComposerDrafts()
    val pinDismissals = PinDismissals()

    var viewing: String? = null

    private val mutableOpenRequests = MutableSharedFlow<String>(extraBufferCapacity = 1)
    val openRequests: SharedFlow<String> = mutableOpenRequests.asSharedFlow()
    private val mutableScreenRequests = MutableSharedFlow<String>(extraBufferCapacity = 1)
    val screenRequests: SharedFlow<String> = mutableScreenRequests.asSharedFlow()

    val state: StateFlow<AppState> = mutableState.asStateFlow()
    val problem: StateFlow<String?> = mutableProblem.asStateFlow()
    private val mutableRetryable = MutableStateFlow<(suspend () -> Unit)?>(null)
    val retryable: StateFlow<(suspend () -> Unit)?> = mutableRetryable.asStateFlow()
    private val mutableLoaded = MutableStateFlow<Set<String>>(emptySet())
    private val mutableLoadFailures = MutableStateFlow<Set<String>>(emptySet())
    val loadFailures: StateFlow<Set<String>> = mutableLoadFailures.asStateFlow()
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
            state.collect { app ->
                voice.observe(app)
                app.assistants.values.firstOrNull { it.builtinKey == VoiceAssistantKey }?.let { voice.useVoice(it.preferences.voiceName, it.preferences.speechSpeed) }
            }
        }

        scope.launch {
            var voiceConversation: String? = null

            voice.state.collect { held ->
                val id = held.turn.conversationId

                if (id != null && id != voiceConversation) {
                    voiceConversation = id
                    act { open(id) }
                }
            }
        }

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

        if (message is StreamMessage.AssistantActionsChanged) {
            act { refreshAssistantActions(message.assistantId) }
        }

        if (message is StreamMessage.ScreenOpened) {
            mutableScreenRequests.tryEmit(message.route)
        }

        if (message is StreamMessage.WellnessChanged) {
            act { refreshGroups() }
            wellness.reload()
            learning.reload()
        }

        announceFinished(before, message)
    }

    fun openScreen(route: String) {
        mutableScreenRequests.tryEmit(route)
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

        val ping = updated.assistantId != null

        val closed = hoursView().closed(updated.groupId)

        if (if (ping) wellness.holdsPings(closed) else wellness.holdsNotifications(closed)) {
            wellness.hold(HeldNotification(updated.title, updated.id, updated.groupId), ping)

            return
        }

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
        refreshExperiments()
        wellness.load()
        voice.load()
        val conversations = api.conversations(archived = false)
        val nodes = api.nodes()

        mutableState.update { it.copy(conversations = conversations.associateBy { c -> c.id }, nodes = nodes.associateBy { n -> n.id }) }
    }

    suspend fun refreshNodes() {
        val nodes = api.nodes()

        mutableState.update { it.copy(nodes = nodes.associateBy { n -> n.id }) }
    }

    suspend fun refreshExperiments() {
        val experiments = orNull { api.experiments() } ?: Experiments()

        mutableState.update { it.copy(experiments = experiments) }
    }

    fun updateExperiments(change: (Experiments) -> Experiments) {
        act {
            experimentsLock.withLock {
                val before = state.value.experiments
                val changed = change(before)

                mutableState.update { it.copy(experiments = changed) }

                try {
                    val saved = api.saveExperiments(changed)

                    mutableState.update { it.copy(experiments = saved) }
                } catch (failure: Exception) {
                    mutableState.update { it.copy(experiments = before) }
                    throw failure
                }
            }
        }
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

    suspend fun refreshRepositories() {
        val forges = api.forges()
        val listing = api.repositories()

        mutableState.update { it.copy(forges = forges, repositoryListing = listing) }
    }

    suspend fun refreshGroups() {
        val groups = api.groups()

        mutableState.update { it.copy(groups = groups.associateBy { group -> group.id }) }
    }

    suspend fun saveGroupHours(id: String, hours: Hours) {
        val group = api.saveGroupHours(id, hours)

        mutableState.update { it.copy(groups = it.groups + (group.id to group)) }
    }

    suspend fun openGroupOutOfHours(id: String, reason: String) {
        api.openGroup(id, reason)
        refreshGroups()
        wellness.reload()
    }

    suspend fun closeGroupOpening(id: String) {
        api.closeGroupOpening(id)
        refreshGroups()
        wellness.reload()
    }

    fun hoursView(now: Instant = Clock.System.now()): HoursView = hoursViewOf(wellness.state.value.settings, state.value.groups.values, now, localTimeOf(platform, now))

    suspend fun refreshMemoryBanks() {
        val banks = orNull { api.memoryBanks() } ?: return

        mutableState.update { it.copy(memoryBanks = banks.associateBy { bank -> bank.id }) }
    }

    suspend fun loadAssistants() {
        val assistants = api.assistants()

        mutableState.update { it.copy(assistants = assistants.associateBy { assistant -> assistant.id }) }
    }

    suspend fun refreshAssistants() {
        val assistants = orNull { api.assistants() } ?: return

        mutableState.update { it.copy(assistants = assistants.associateBy { assistant -> assistant.id }) }
    }

    suspend fun agentSettings(): mycelium.api.AgentSettings? = orNull { api.agentSettings() }

    suspend fun accountPrices(): List<mycelium.api.AccountPrice>? = orNull { api.accountPrices() }

    suspend fun refreshAssistantActions(assistantId: String) {
        val actions = orNull { api.assistantActions(assistantId) } ?: return

        mutableState.update { it.copy(assistantActions = it.assistantActions + (assistantId to actions)) }
    }

    suspend fun refreshActionProposals(assistantId: String) {
        val proposals = orNull { api.actionProposals(assistantId) } ?: return

        mutableState.update { it.copy(actionProposals = it.actionProposals + proposals.associateBy { proposal -> proposal.id }) }
    }

    suspend fun refreshAssistantChanges() {
        val changes = orNull { api.assistantChanges() } ?: return

        mutableState.update { it.copy(assistantChanges = it.assistantChanges + changes.associateBy { change -> change.id }) }
    }

    suspend fun createAssistant(request: mycelium.api.CreateAssistantRequest): mycelium.api.Assistant {
        val assistant = api.createAssistant(request)

        mutableState.update { it.copy(assistants = it.assistants + (assistant.id to assistant)) }

        return assistant
    }

    suspend fun patchAssistant(assistant: mycelium.api.Assistant, request: mycelium.api.PatchAssistantRequest): mycelium.api.Assistant {
        val changed = api.patchAssistant(assistant.id, request)

        mutableState.update { it.copy(assistants = it.assistants + (changed.id to changed)) }

        return changed
    }

    suspend fun resetAssistantInstructions(assistant: mycelium.api.Assistant): mycelium.api.Assistant {
        val reset = api.resetAssistantInstructions(assistant.id)

        mutableState.update { it.copy(assistants = it.assistants + (reset.id to reset)) }

        return reset
    }

    fun promptAssistant(assistant: mycelium.api.Assistant, text: String, spoken: Boolean = false) {
        val prompt = text.trim()

        mutableState.update { it.copy(sending = it.sending + (assistant.id to it.sending[assistant.id].orEmpty() + prompt)) }

        act {
            try {
                api.promptAssistant(assistant.id, prompt, spoken)
                refreshAssistants()
            } finally {
                mutableState.update { it.copy(sending = it.sending.withoutFirst(assistant.id, prompt)) }
            }
        }
    }

    fun clearAssistant(assistant: mycelium.api.Assistant) {
        act {
            val cleared = api.clearAssistant(assistant.id)

            mutableState.update { it.copy(assistants = it.assistants + (cleared.id to cleared)) }
        }
    }

    suspend fun createProject(request: CreateProjectRequest): Project {
        val project = api.createProject(request)

        mutableState.update { it.copy(projects = it.projects + (project.id to project)) }

        return project
    }

    suspend fun directories(nodeId: String, path: String): Directories = api.directories(nodeId, path)

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
        val goal = api.setGoal(conversationId, SetGoalRequest(text, limits.tokenBudget, limits.timeLimitSeconds, limits.fiveHourPercent, limits.weeklyPercent, limits.repeatLimit))

        mutableState.update { it.copy(goals = it.goals + (conversationId to goal)) }
    }

    suspend fun pauseGoal(conversationId: String, paused: Boolean) {
        val goal = api.pauseGoal(conversationId, paused)

        mutableState.update { it.copy(goals = it.goals + (conversationId to goal)) }
    }

    suspend fun sendGoalAgain(conversationId: String) {
        val goal = api.sendGoalAgain(conversationId)

        mutableState.update { it.copy(goals = it.goals + (conversationId to goal)) }
    }

    suspend fun nudgeGoal(conversationId: String) {
        val goal = api.nudgeGoal(conversationId)

        mutableState.update { it.copy(goals = it.goals + (conversationId to goal)) }
    }

    suspend fun continueGoal(conversationId: String, note: String?) {
        val goal = api.continueGoal(conversationId, note)

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

    suspend fun declineInstructionProposal(proposal: InstructionProposal, note: String?, redo: Boolean, notify: Boolean = true) {
        val declined = api.declineInstructionProposal(proposal.id, DeclineInstructionProposalRequest(note?.trim()?.takeIf { it.isNotEmpty() }, redo, notify))

        mutableState.update { it.copy(instructionProposals = it.instructionProposals + (declined.id to declined)) }
    }

    suspend fun refreshConversationConnections() {
        val connections = api.conversationConnections()

        mutableState.update { it.copy(conversationConnections = connections.associateBy { connection -> connection.id }) }
    }

    suspend fun revokeConversationConnection(id: String) {
        api.revokeConversationConnection(id)
        mutableState.update { it.copy(conversationConnections = it.conversationConnections - id) }
    }

    suspend fun refreshGroupConnections() {
        val connections = api.groupConnections()

        mutableState.update { it.copy(groupConnections = connections.associateBy { connection -> connection.id }) }
    }

    suspend fun connectGroups(request: CreateGroupConnectionRequest) {
        val connection = api.connectGroups(request)

        mutableState.update { it.copy(groupConnections = it.groupConnections + (connection.id to connection)) }
    }

    suspend fun revokeGroupConnection(id: String) {
        api.revokeGroupConnection(id)
        mutableState.update { it.copy(groupConnections = it.groupConnections - id) }
    }

    suspend fun approveAndConnectMessageProposal(proposal: MessageProposal) {
        val sent = api.approveAndConnectMessageProposal(proposal.id)

        mutableState.update { it.copy(messageProposals = it.messageProposals + (sent.id to sent)) }
        refreshConversationConnections()
    }

    suspend fun approveAndConnectGroupsMessageProposal(proposal: MessageProposal) {
        val sent = api.approveAndConnectGroups(proposal.id)

        mutableState.update { it.copy(messageProposals = it.messageProposals + (sent.id to sent)) }
        refreshGroupConnections()
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

    suspend fun refreshStartProposals() {
        val proposals = api.startProposals()

        mutableState.update { it.copy(startProposals = proposals.associateBy { proposal -> proposal.id }) }
    }

    suspend fun approveStartProposal(proposal: StartProposal, choices: Map<String, StartProposalChoice>) {
        val started = api.approveStartProposal(proposal.id, choices)

        mutableState.update { it.copy(startProposals = it.startProposals + (started.id to started)) }
    }

    suspend fun connectStartProposal(proposal: StartProposal, choices: Map<String, StartProposalChoice>) {
        val started = api.approveAndConnectStartProposal(proposal.id, choices)

        mutableState.update { it.copy(startProposals = it.startProposals + (started.id to started)) }
        refreshConversationConnections()
    }

    suspend fun declineStartProposal(proposal: StartProposal) {
        val declined = api.declineStartProposal(proposal.id)

        mutableState.update { it.copy(startProposals = it.startProposals + (declined.id to declined)) }
    }

    suspend fun refreshArtifactAccessRequests() {
        val requests = api.artifactAccessRequests()
        mutableState.update { it.copy(artifactAccessRequests = requests.associateBy { request -> request.id }) }
    }

    suspend fun decideArtifactAccessRequest(request: ArtifactAccessRequest, approve: Boolean, project: Boolean = false) {
        val decided = if (approve) api.approveArtifactAccessRequest(request.id, project) else api.declineArtifactAccessRequest(request.id)
        mutableState.update { it.copy(artifactAccessRequests = it.artifactAccessRequests + (decided.id to decided)) }
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

    fun shareChatToolServers(servers: List<mycelium.api.ChatToolServer>) {
        mutableState.update { it.copy(chatToolServers = servers) }
    }

    suspend fun refreshChatToolServers() {
        val servers = api.chatToolServers()

        shareChatToolServers(servers)
    }

    suspend fun refreshApps() {
        val apps = api.apps()

        mutableState.update { it.copy(apps = apps) }
    }

    suspend fun createArtifact(request: CreateArtifactRequest): Artifact {
        val created = api.createArtifact(request)

        mutableState.update { it.copy(artifacts = it.artifacts + (created.id to created)) }

        return created
    }

    suspend fun renameArtifact(artifact: Artifact, title: String) {
        val changed = api.editArtifact(artifact.id, PatchArtifactRequest(title = title))

        mutableState.update { it.copy(artifacts = it.artifacts + (changed.id to changed)) }
    }

    suspend fun setArtifactDesignSystem(artifact: Artifact, designSystemId: String?) {
        val changed = api.editArtifact(artifact.id, PatchArtifactRequest(designSystemId = designSystemId.orEmpty()))

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

    suspend fun refreshWorkflows() {
        val workflows = api.workflows()
        mutableState.update { it.copy(workflows = workflows.associateBy { workflow -> workflow.id }) }
    }

    suspend fun refreshWorkflow(id: String) {
        val workflow = api.workflow(id)

        mutableState.update { it.copy(workflows = it.workflows + (workflow.id to workflow)) }
    }

    suspend fun controlWorkflow(id: String, request: mycelium.api.WorkflowControlRequest) {
        val workflow = api.controlWorkflow(id, request)

        mutableState.update { it.copy(workflows = it.workflows + (workflow.id to workflow)) }
    }

    suspend fun rerunWorkflow(id: String): String {
        val workflow = api.rerunWorkflow(id)

        mutableState.update { it.copy(workflows = it.workflows + (workflow.id to workflow)) }

        return workflow.id
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
            val page = api.events(conversationId, before = oldest, limit = eventPageSize)

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
                val page = api.events(conversationId, after, eventPageSize)
                fetched += page

                if (page.size < eventPageSize) {
                    break
                }

                after = page.maxOf { it.seq }
            }
        } else {
            fetched += api.events(conversationId, before = Long.MAX_VALUE, limit = eventPageSize)
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

        loadEnvironment(conversationId)

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
                val queuedNotice = api.sendPrompt(conversationId, prompt, attachments)

                if (queuedNotice != null) {
                    mutableProblem.value = queuedNotice
                }
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

    fun iterateWalkthrough(conversationId: String, comment: String) {
        val scope = mutableWalkthroughScopes.value[conversationId] ?: WalkthroughScope.Branch
        val revising = comment.isNotEmpty() || mutableWalkthroughNotes.value[conversationId].orEmpty().any { !it.sent }
        val prompt = listOf(iterateWalkthroughPrompt(scope, revising), comment).filter { it.isNotEmpty() }.joinToString("\n\n")

        send(conversationId, withWalkthroughNotes(conversationId, prompt))
    }

    fun addWalkthroughNote(conversationId: String, walkthroughId: String, request: WalkthroughNoteRequest) {
        act {
            val note = api.addWalkthroughNote(walkthroughId, request)
            mutableWalkthroughNotes.update { it + (conversationId to it[conversationId].orEmpty().filter { held -> held.id != note.id } + note) }
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

    private val avatarCache = mutableMapOf<Pair<String, String>, ByteArray>()

    suspend fun forgeAvatarBytes(forgeId: String, url: String): ByteArray? {
        val key = forgeId to url
        avatarCache[key]?.let { return it }

        val bytes = orNull { api.forgeAvatar(forgeId, url) } ?: return null

        if (avatarCache.size >= MaxCachedAvatars) {
            avatarCache.clear()
        }

        avatarCache[key] = bytes

        return bytes
    }

    suspend fun attachmentBytes(sha256: String): ByteArray? {
        attachmentCache[sha256]?.let { return it }

        val bytes = orNull { api.attachmentBytes(sha256) } ?: return null

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

        val bytes = orNull { fetch() } ?: return null

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

    suspend fun loadEnvironment(conversationId: String) {
        val environment = orNull { api.environment(conversationId) }
        val actions = if (environment == null) emptyList() else orNull { api.environmentActions(conversationId) }.orEmpty()

        mutableState.update {
            it.copy(
                environments = if (environment == null) it.environments - conversationId else it.environments + (conversationId to environment),
                environmentActions = it.environmentActions + (conversationId to actions),
            )
        }
    }

    suspend fun loadLastEnvironmentNode() {
        val last = orNull { api.lastEnvironmentNode() }

        mutableState.update { it.copy(lastEnvironmentNodeId = last?.nodeId) }
    }

    fun startEnvironment(conversationId: String, apps: List<String>? = null, nodeId: String? = null) {
        act {
            val environment = api.startEnvironment(conversationId, StartEnvironmentRequest(apps = apps, nodeId = nodeId))

            mutableState.update { it.copy(environments = it.environments + (conversationId to environment), environmentActions = it.environmentActions - conversationId, lastEnvironmentNodeId = environment.nodeId) }
        }
    }

    fun stopEnvironment(conversationId: String) {
        act {
            api.stopEnvironment(conversationId)
            loadEnvironment(conversationId)
        }
    }

    fun takeOverEnvironment(conversationId: String) {
        act {
            val environment = api.takeOverEnvironment(conversationId)

            mutableState.update { it.copy(environments = it.environments + (conversationId to environment)) }
        }
    }

    fun handBackEnvironment(conversationId: String) {
        act {
            val environment = api.handBackEnvironment(conversationId)

            mutableState.update { it.copy(environments = it.environments + (conversationId to environment)) }
        }
    }

    fun stopBrowserTask(conversationId: String) {
        act {
            val environment = api.stopBrowserTask(conversationId)

            mutableState.update { it.copy(environments = it.environments + (conversationId to environment)) }
        }
    }

    fun openEnvironmentApp(conversationId: String, app: String) {
        act {
            val environment = api.openEnvironmentApp(conversationId, app)

            mutableState.update {
                val recent = (listOf(app) + it.recentEnvironmentApps[conversationId].orEmpty().filter { id -> id != app }).take(EnvironmentRecentAppsShown)

                it.copy(environments = it.environments + (conversationId to environment), recentEnvironmentApps = it.recentEnvironmentApps + (conversationId to recent))
            }
        }
    }

    fun focusEnvironmentWindow(conversationId: String, window: Long) {
        act {
            val environment = api.focusEnvironmentWindow(conversationId, window)

            mutableState.update { it.copy(environments = it.environments + (conversationId to environment)) }
        }
    }

    fun checkEnvironment(nodeId: String) {
        act {
            val capability = api.checkEnvironment(nodeId)

            if (capability != null) {
                mutableState.update { held -> held.nodes[nodeId]?.let { node -> held.copy(nodes = held.nodes + (nodeId to node.copy(environment = capability))) } ?: held }
            }
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

    fun setConversationProject(conversation: Conversation, projectId: String?) {
        mutableState.update { it.copy(conversations = it.conversations + (conversation.id to conversation.copy(projectId = projectId))) }

        act {
            try {
                val request = if (projectId == null) PatchConversationRequest(clearProject = true) else PatchConversationRequest(projectId = projectId)
                val moved = api.patchConversation(conversation.id, request)
                mutableState.update { it.copy(conversations = it.conversations + (moved.id to moved)) }
            } catch (failure: Exception) {
                mutableState.update { it.copy(conversations = it.conversations + (conversation.id to conversation)) }
                throw failure
            }
        }
    }

    suspend fun continueOn(id: String, choice: String) {
        val conversation = mutableState.value.conversations[id] ?: return
        val request = when {
            choice.startsWith(RoutingPrefix) -> ContinueRequest(conversation.agent, routing = choice.removePrefix(RoutingPrefix))
            choice.startsWith(ContinueAccountPrefix) -> accountRequest(conversation.agent, choice.removePrefix(ContinueAccountPrefix))
            choice.startsWith(ContinueAgentPrefix) -> choice.removePrefix(ContinueAgentPrefix).let { accountRequest(it.substringBefore(':'), it.substringAfter(':')) }
            choice.startsWith(ContinueNodePrefix) -> choice.removePrefix(ContinueNodePrefix).split(':', limit = 3).let { accountRequest(it[1], it[2]).copy(nodeId = it[0]) }
            choice.startsWith(ContinueChatPrefix) -> choice.removePrefix(ContinueChatPrefix).let { ContinueRequest("chat", providerId = it.substringBefore(':'), model = it.substringAfter(':')) }
            else -> return
        }
        val continued = api.continueConversation(id, request)

        mutableState.update { it.copy(conversations = it.conversations + (continued.id to continued)) }
    }

    private fun accountRequest(agent: String, account: String): ContinueRequest =
        if (account == DefaultAccountId) ContinueRequest(agent, useDefaultAccount = true) else ContinueRequest(agent, account = account)

    fun closeSession(conversation: Conversation) {
        act { api.closeSession(conversation.id) }
    }

    fun dismissFailure(conversation: Conversation) {
        act { api.dismissConversation(conversation.id) }
    }

    fun act(block: suspend () -> Unit) {
        scope.launch {
            mutableProblem.value = null
            mutableRetryable.value = null
            guarded(block)
        }
    }

    fun load(list: String? = null, block: suspend () -> Unit) {
        scope.launch {
            mutableProblem.value = null
            mutableRetryable.value = null

            try {
                block()
                list?.let { name ->
                    mutableLoaded.update { it + name }
                    mutableLoadFailures.update { it - name }
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                val firstLoad = list != null && list !in mutableLoaded.value

                if (firstLoad) {
                    mutableLoadFailures.update { it + list.orEmpty() }
                } else {
                    mutableProblem.value = (failure as? ApiException)?.message ?: ControlPlaneUnreachable
                    mutableRetryable.value = block
                }
            }
        }
    }

    fun retryProblem() {
        val block = mutableRetryable.value ?: return

        load(block = block)
    }

    private suspend fun guarded(block: suspend () -> Unit) {
        try {
            block()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: ApiException) {
            mutableProblem.value = failure.message
        } catch (_: Exception) {
            mutableProblem.value = ControlPlaneUnreachable
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
        mutableRetryable.value = null
    }
}
