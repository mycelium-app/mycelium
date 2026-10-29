package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class Extensions(
    @SerialName("generated_ui") val generatedUi: Boolean,
    @SerialName("rich_markdown") val richMarkdown: Boolean,
    @SerialName("mycelium_tools") val myceliumTools: Boolean,
)

@Serializable
data class Experiments(
    val voice: Boolean = false,
    @SerialName("kubernetes_nodes") val kubernetesNodes: Boolean = false,
    @SerialName("tasks_and_workflows") val tasksAndWorkflows: Boolean = false,
    val history: Boolean = false,
)
