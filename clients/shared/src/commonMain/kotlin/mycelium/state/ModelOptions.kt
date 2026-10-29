package mycelium.state

import mycelium.api.Conversation
import mycelium.api.ModelChoice
import mycelium.api.ModelProvider
import mycelium.api.NodeSummary
import mycelium.api.Routing

data class ModelOption(
    val agent: String?,
    val providerId: String?,
    val model: String,
    val name: String,
    val available: Boolean = true,
    val where: String? = null,
) {
    val key: String = "${agent.orEmpty()}|${providerId.orEmpty()}|$model"

    val harness: String = agent?.let(::agentName) ?: "Chat"

    fun choice(account: String? = null): ModelChoice = ModelChoice(agent = agent, providerId = providerId, model = model.ifEmpty { null }, account = account)

    fun isChosen(choice: ModelChoice): Boolean = !choice.anyModel && choice.agent == agent && choice.providerId == providerId && (choice.model ?: "") == model
}

fun modelOptions(nodes: Collection<NodeSummary>, node: NodeSummary?, providers: List<ModelProvider>): List<ModelOption> {
    val agents = nodes.flatMap { held -> held.agents.filter { it.available }.map { it.agent } }.distinct()
    val harness = agents.flatMap { agent ->
        val reported = nodes.flatMap { held -> held.harnessModels.filter { it.agent == agent }.flatMap { report -> report.models.map { held to it } } }
        val holders = reported.groupBy({ it.second.id }, { it.first })
        val names = reported.associate { it.second.id to it.second.name }

        if (holders.isEmpty()) {
            listOf(ModelOption(agent, null, DefaultModelId, agentName(agent), node == null || node.agents.any { it.agent == agent && it.available }))
        } else {
            holders.map { (id, held) ->
                val here = node == null || held.any { it.id == node.id }

                ModelOption(agent, null, id, names.getValue(id), here, if (here) null else "On ${held.minOf { it.name }}")
            }
        }
    }
    val chat = providers.flatMap { provider -> provider.models.map { ModelOption(null, provider.id, it, it) } }

    return harness + chat
}

fun List<ModelChoice>.favouriteOptions(options: List<ModelOption>, providers: List<ModelProvider>): List<ModelOption> =
    filterNot { it.anyModel }.map { favourite ->
        val held = favourite.copy(account = null)

        options.firstOrNull { it.isChosen(held) }
            ?: ModelOption(favourite.agent, favourite.providerId, favourite.model.orEmpty(), favourite.model?.takeIf { favourite.agent == null } ?: modelChoiceName(held, providers), available = false)
    }

fun List<ModelChoice>.isFavourite(option: ModelOption): Boolean = any { option.isChosen(it.copy(account = null)) }

fun List<ModelChoice>.withFavourite(option: ModelOption, on: Boolean): List<ModelChoice> {
    val without = filterNot { option.isChosen(it.copy(account = null)) }

    return if (on) without + option.choice() else without
}

fun List<ModelOption>.matching(query: String): List<ModelOption> {
    val terms = query.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }

    return filter { option -> terms.all { term -> listOf(option.name, option.harness, option.model).any { it.contains(term, ignoreCase = true) } } }
}

fun anyModelHint(favourites: List<ModelOption>): String =
    favourites.take(2).joinToString(", then ") { it.name }.ifEmpty { "Add favourites in Settings, Agents" }

fun conversationModelLabel(conversation: Conversation, currentModel: String?, options: List<ModelOption>): String {
    if (conversation.routing == Routing.AnyModel) {
        return "Any model"
    }

    val held = ModelChoice(agent = conversation.agent.takeIf { it != "chat" }, providerId = conversation.providerId, model = (if (conversation.agent == "chat") conversation.model else currentModel)?.ifEmpty { null })
    val name = options.firstOrNull { it.isChosen(held) }?.name ?: held.model ?: if (conversation.agent == "chat") "Choose a model" else agentName(conversation.agent)
    val pinned = conversation.account.takeIf { conversation.routing == null && conversation.agent != "chat" }

    return listOfNotNull(name, pinned?.substringBefore('@')).joinToString(" · ")
}

fun ModelChoice.displayName(options: List<ModelOption>, providers: List<ModelProvider>): String {
    if (anyModel) {
        return "Any model"
    }

    val held = copy(account = null)

    return options.firstOrNull { it.isChosen(held) }?.name ?: modelChoiceName(held, providers)
}
