package mycelium.ui

import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.put
import mycelium.api.ModelProvider
import mycelium.state.AppState
import mycelium.state.workflowInGroup
import mycelium.state.Store
import mycelium.state.sessionOptions
import mycelium.state.agentName

private data class WorkflowDraft(
    val title: String = "",
    val prompt: String = "",
    val node: String = "",
    val cwd: String = "",
    val projectId: String = "",
    val branch: String = "",
    val chat: Boolean = false,
    val providerId: String = "",
    val agent: String = "codex",
    val model: String = "",
    val difficulty: String = "",
    val dependsOn: Set<Int> = emptySet(),
    val gate: Boolean = false,
    val nested: JsonObject? = null,
)

private const val WorkflowsList = "workflows"

@Composable
fun WorkflowsScreen(store: Store, state: AppState, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    val group = LocalGroupFilter.current
    val workflows = state.workflows.values.filter { workflowInGroup(it, state, group) }
    var selected by remember { mutableStateOf<String?>(null) }
    var query by remember { mutableStateOf("") }
    val loadFailures by store.loadFailures.collectAsState()
    val load = { store.load(WorkflowsList) { store.refreshWorkflows() } }
    var providers by remember { mutableStateOf<List<ModelProvider>>(emptyList()) }
    var creating by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var title by remember { mutableStateOf("") }
    var drafts by remember { mutableStateOf(listOf(WorkflowDraft(node = state.nodes.keys.firstOrNull().orEmpty()))) }
    var rounds by remember { mutableStateOf("1") }
    var stopWhen by remember { mutableStateOf("") }
    var gateDecisions by remember { mutableStateOf<Map<String, String>>(emptyMap()) }

    LaunchedEffect(store) {
        load()
    }

    LaunchedEffect(creating) {
        if (creating) {
            store.act {
                providers = store.api.modelProviders()
                store.refreshProjects()
            }
        }
    }

    val control: (String, String) -> Unit = { workflowId, action ->
        scope.launch {
            try {
                store.api.controlWorkflow(workflowId, buildJsonObject { put("action", action) })
                store.refreshWorkflows()
            } catch (failure: Exception) {
                error = failure.message
            }
        }
    }

    val selectedWorkflow = workflows.firstOrNull { it["id"]?.jsonPrimitive?.content == selected }

    Column(Modifier.fillMaxSize().background(colors.bgContent)) {
        Bar {
            if (selectedWorkflow != null) {
                BackButton("workflows", { selected = null })
            }

            BarTitle(selectedWorkflow?.get("title")?.jsonPrimitive?.content ?: "Workflows")
            Box(Modifier.weight(1f))
            MyButton({ creating = !creating }, text = "New workflow", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
        }

        Column(Modifier.fillMaxWidth().weight(1f).verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            error?.let { Label(it, color = colors.textMuted) }

            if (selected == null && !creating) {
                if (WorkflowsList in loadFailures && state.workflows.isEmpty()) {
                    LoadFailedState("workflows", load)
                } else if (workflows.isEmpty()) {
                    EmptyState("No workflows. Ask an agent to start one, or choose New workflow.")
                } else {
                    ListSearch(query, { query = it }, "workflows")
                }
            }

            if (creating) {
                Label("New workflow", style = Tokens.Type.MetaStrong)
                WorkflowField("Title", title, { title = it })
                WorkflowField("Maximum rounds (1 to 20)", rounds, { rounds = it })
                WorkflowField("Stop when every result contains (optional)", stopWhen, { stopWhen = it })
                for ((index, draft) in drafts.withIndex()) {
                    Column(Modifier.fillMaxWidth().background(colors.bgPanel, BlockShape).border(Tokens.Size.Hairline, colors.border, BlockShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Label("Step ${index + 1}", style = Tokens.Type.MetaStrong)
                        WorkflowField("Step ${index + 1} title", draft.title, { value -> drafts = drafts.toMutableList().also { it[index] = draft.copy(title = value) } })
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(gate = false, nested = null) } }, text = "Agent", selected = !draft.gate && draft.nested == null)
                            MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(gate = true, nested = null) } }, text = "Human decision", selected = draft.gate)
                        }

                        if (!draft.gate && draft.nested == null) {
                            WorkflowField("Step ${index + 1} prompt", draft.prompt, { value -> drafts = drafts.toMutableList().also { it[index] = draft.copy(prompt = value) } }, multiline = true)
                            Label("Use {{step1.output}} to include another step’s result, or {{previous}} for the previous round.", color = colors.textMuted)
                            Segmented(listOf("Workspace agent", "Chat model"), if (draft.chat) "Chat model" else "Workspace agent", { value -> drafts = drafts.toMutableList().also { it[index] = draft.copy(chat = value == "Chat model", model = "", difficulty = "") } }, groupLabel = "Step ${index + 1} runtime")

                            if (!draft.chat) {
                            WorkflowField("Step ${index + 1} folder", draft.cwd, { value -> drafts = drafts.toMutableList().also { it[index] = draft.copy(cwd = value) } })
                            for (project in state.projects.values.filter { it.nodeId == draft.node }) {
                                MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(projectId = project.id, cwd = project.path) } }, text = project.name, selected = draft.projectId == project.id)
                            }

                            if (draft.projectId.isNotBlank()) {
                                WorkflowField("Step ${index + 1} worktree branch (optional)", draft.branch, { value -> drafts = drafts.toMutableList().also { it[index] = draft.copy(branch = value) } })
                            }
                            Label("Node", color = colors.textMuted)
                            for ((nodeId, availableNode) in state.nodes) {
                                MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(node = nodeId) } }, text = availableNode.hostname, selected = draft.node == nodeId)
                            }

                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                for (agent in listOf("claude", "codex")) {
                                    MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(agent = agent) } }, text = if (agent == "claude") "Claude" else "Codex", selected = draft.agent == agent)
                                }
                            }

                            val available = state.conversations.values.filter { it.nodeId == draft.node && it.agent == draft.agent }.map { sessionOptions(it, emptyList()) }
                            val models = available.flatMap { it.models }.distinctBy { it.id }
                            val difficulties = available.flatMap { it.settings }.filter { it.category == "thought_level" || it.id.contains("effort") || it.id.contains("reasoning") }.flatMap { it.choices }.distinctBy { it.id }
                            Label("Model", color = colors.textMuted)
                            MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(model = "") } }, text = "Agent default", selected = draft.model.isBlank())
                            for (model in models) {
                                MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(model = model.id) } }, text = model.name, selected = draft.model == model.id)
                            }

                            Label("Difficulty", color = colors.textMuted)
                            MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(difficulty = "") } }, text = "Agent default", selected = draft.difficulty.isBlank())
                            for (difficulty in difficulties) {
                                MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(difficulty = difficulty.id) } }, text = difficulty.name, selected = draft.difficulty == difficulty.id)
                            }
                            } else {
                                if (providers.isEmpty()) {
                                    Label("Add a chat model provider in Settings before starting this step.", color = colors.textMuted)
                                }

                                for (provider in providers) {
                                    MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(providerId = provider.id, model = provider.defaultModel.orEmpty()) } }, text = provider.name, selected = draft.providerId == provider.id)
                                }

                                val provider = providers.firstOrNull { it.id == draft.providerId }

                                for (model in provider?.models.orEmpty()) {
                                    MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(model = model) } }, text = model, selected = draft.model == model)
                                }
                            }
                        }

                        if (draft.nested != null) {
                            Label("Nested workflow: ${draft.nested["title"]?.jsonPrimitive?.content.orEmpty()}")
                        }

                        if (workflows.isNotEmpty()) {
                            Label("Or run an existing workflow", color = colors.textMuted)
                            for (existing in workflows.filter { it["parent_id"] == JsonNull }) {
                                MyButton({ drafts = drafts.toMutableList().also { it[index] = draft.copy(gate = false, nested = existing["plan"]!!.jsonObject) } }, text = existing["title"]!!.jsonPrimitive.content)
                            }
                        }

                        if (index > 0) {
                            Label("Wait for", color = colors.textMuted)
                            for (predecessor in 0 until index) {
                                MyButton({
                                    val dependencies = if (predecessor in draft.dependsOn) draft.dependsOn - predecessor else draft.dependsOn + predecessor
                                    drafts = drafts.toMutableList().also { it[index] = draft.copy(dependsOn = dependencies) }
                                }, text = drafts[predecessor].title.ifBlank { "Step ${predecessor + 1}" }, selected = predecessor in draft.dependsOn)
                            }
                        }
                    }
                }

                MyButton({ drafts = drafts + WorkflowDraft(node = state.nodes.keys.firstOrNull().orEmpty()) }, text = "Add step", icon = MyIcon.Plus)
                MyButton({
                    scope.launch {
                        try {
                            val plan = buildJsonObject {
                                put("title", title)
                                put("max_rounds", rounds.toIntOrNull() ?: 0)
                                if (stopWhen.isNotBlank()) { put("stop_when", stopWhen) }
                                put("steps", JsonArray(drafts.mapIndexed { index, draft ->
                                    buildJsonObject {
                                        put("key", "step${index + 1}")
                                        put("title", draft.title)
                                        put("prompt", draft.prompt)
                                        put("gate", draft.gate)
                                        put("depends_on", JsonArray(draft.dependsOn.sorted().map { JsonPrimitive("step${it + 1}") }))
                                        if (draft.nested != null) {
                                            put("workflow", draft.nested)
                                        } else if (!draft.gate) {
                                            if (draft.chat) {
                                                put("provider_id", draft.providerId)
                                            } else {
                                                put("node_id", draft.node)
                                                put("agent", draft.agent)
                                                put("cwd", draft.cwd)

                                                if (draft.projectId.isNotBlank()) {
                                                    put("project_id", draft.projectId)
                                                }

                                                if (draft.branch.isNotBlank()) {
                                                    put("branch", draft.branch)
                                                }
                                            }
                                            if (draft.model.isNotBlank()) { put("model", draft.model) }
                                            if (draft.difficulty.isNotBlank()) { put("difficulty", draft.difficulty) }
                                        }
                                    }
                                }))
                            }
                            val workflow = store.api.createWorkflow(plan)
                            store.refreshWorkflows()
                            selected = workflow["id"]?.jsonPrimitive?.content
                            creating = false
                            error = null
                        } catch (failure: Exception) {
                            error = failure.message
                        }
                    }
                }, text = "Start workflow", variant = ButtonVariant.Outline)
            }

            for (workflow in workflows.filter { if (selected == null) it["title"]?.jsonPrimitive?.content.orEmpty().contains(query.trim(), ignoreCase = true) else it["id"]?.jsonPrimitive?.content == selected }) {
                val id = workflow["id"]!!.jsonPrimitive.content
                val status = workflow["status"]!!.jsonPrimitive.content
                val name = workflow["title"]!!.jsonPrimitive.content
                if (selected != id) {
                    val menu = rememberRowMenuState()

                    Row(Modifier.rowContextMenu(menu), verticalAlignment = Alignment.CenterVertically) {
                        MyButton({ selected = id }, text = "$name · $status · Round ${workflow["round"]}")

                        RowMenu(name, menu) { close ->
                            MenuItem("Open", onClick = { close(); selected = id }, leading = MyIcon.Layers)

                            if (status == "running" || status == "paused") {
                                MenuItem(if (status == "paused") "Resume" else "Pause", onClick = { close(); control(id, if (status == "paused") "resume" else "pause") }, leading = MyIcon.Clock)
                                MenuItem("Stop", onClick = { close(); control(id, "stop") }, leading = MyIcon.Stop)
                            }
                        }
                    }

                    continue
                }

                Label("$status · Round ${workflow["round"]}", style = Tokens.Type.Meta, color = colors.textMuted)

                if (status == "failed") {
                    Label(workflow["output"]?.jsonPrimitive?.content.orEmpty())
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for ((action, label) in listOf(if (status == "paused") "resume" to "Resume workflow" else "pause" to "Pause workflow", "stop" to "Stop workflow")) {
                        MyButton({ control(id, action) }, text = label)
                    }
                }

                val specs = workflow["plan"]!!.jsonObject["steps"]!!.jsonArray
                val steps = workflow["steps"]!!.jsonArray.map { it.jsonObject }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                for ((heading, states) in listOf("Finished" to setOf("done"), "Needs you" to setOf("gate"), "Running" to setOf("running", "starting"), "Waiting" to setOf("waiting"), "Failed" to setOf("failed"))) {
                    val matches = steps.filter { it["status"]!!.jsonPrimitive.content in states }
                    if (matches.isEmpty()) { continue }
                    Column(Modifier.widthIn(max = 310.dp).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Label(heading, style = Tokens.Type.MetaStrong)
                    for (step in matches) {
                        val key = step["key"]!!.jsonPrimitive.content
                        val spec = specs.first { it.jsonObject["key"]!!.jsonPrimitive.content == key }.jsonObject
                        Column(Modifier.fillMaxWidth().background(colors.bgPanel, BlockShape).border(Tokens.Size.Hairline, colors.border, BlockShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Label(spec["title"]!!.jsonPrimitive.content, style = Tokens.Type.MetaStrong)
                            val metadata = listOfNotNull(spec["agent"]?.jsonPrimitive?.content?.let(::agentName), spec["model"]?.jsonPrimitive?.content, spec["difficulty"]?.jsonPrimitive?.content, spec["node_id"]?.jsonPrimitive?.content?.let { state.nodes[it]?.name })

                            if (metadata.isNotEmpty()) {
                                Label(metadata.joinToString(" · "), style = Tokens.Type.Meta, color = colors.textMuted)
                            }

                            if (step["status"]?.jsonPrimitive?.content == "gate") {
                                spec["prompt"]?.jsonPrimitive?.content?.takeIf { it.isNotBlank() }?.let { Label(it) }
                            }

                            spec["depends_on"]?.jsonArray?.takeIf { it.isNotEmpty() }?.let { dependencies ->
                                val names = dependencies.map { dependency -> specs.first { it.jsonObject["key"] == dependency }.jsonObject["title"]!!.jsonPrimitive.content }
                                Label("Uses ${names.joinToString()}", style = Tokens.Type.Meta, color = colors.textMuted)
                            }
                            step["conversation_id"]?.jsonPrimitive?.content?.takeUnless { it == "null" }?.let { conversation ->
                                MyButton({ onOpenConversation(conversation) }, text = "Open conversation")

                                if (step["status"]?.jsonPrimitive?.content in setOf("running", "starting")) {
                                    MyButton({ store.act { store.api.closeSession(conversation) } }, text = "Stop", icon = MyIcon.Stop, description = "Stop ${spec["title"]!!.jsonPrimitive.content}", size = ButtonSize.Sm, muted = true)
                                }
                            }
                            step["workflow_id"]?.jsonPrimitive?.content?.takeUnless { it == "null" }?.let { child -> MyButton({ selected = child }, text = "Open nested workflow") }
                            if (step["status"]!!.jsonPrimitive.content == "gate") {
                                WorkflowField("Decision for $key", gateDecisions["$id/$key"].orEmpty(), { value -> gateDecisions = gateDecisions + ("$id/$key" to value) }, multiline = true)
                                MyButton({
                                    scope.launch {
                                        try {
                                            store.api.controlWorkflow(id, buildJsonObject { put("action", "approve"); put("step", key); put("output", gateDecisions["$id/$key"].orEmpty()) })
                                            store.refreshWorkflows()
                                            gateDecisions = gateDecisions - "$id/$key"
                                        } catch (failure: Exception) {
                                            error = failure.message
                                        }
                                    }
                                }, text = "Approve and continue", variant = ButtonVariant.Outline)
                            }

                            val output = step["output"]?.jsonPrimitive?.content.orEmpty()
                            if (output.isNotBlank()) {
                                Label(output)
                            }
                        }
                    }
                }
                }
                }
            }
        }
    }
}

@Composable
private fun WorkflowField(label: String, value: String, onChange: (String) -> Unit, multiline: Boolean = false) {
    val colors = MyTheme.colors
    if (!multiline) {
        TextInput(value, onChange, label)
        return
    }

    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Label(label, color = colors.textMuted)
        BasicTextField(value, onChange, interactionSource = source, modifier = Modifier.fillMaxWidth().heightIn(min = 80.dp).focusRing(focused, ControlShape, always = true).background(colors.bgPanel).border(1.dp, colors.textMuted).padding(8.dp).semantics { contentDescription = label }, textStyle = Tokens.Type.MetaStrong.copy(color = colors.text), singleLine = false)
    }
}
