package mycelium.state

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import mycelium.api.AppDeployment
import mycelium.api.AppEnv
import mycelium.api.AppSummary
import mycelium.api.ChatToolServer

const val CoolifyKind = "coolify"
const val TandoorKind = "tandoor"
const val BuildLogs = "build"
const val RuntimeLogs = "runtime"

private val DeployTools = setOf("deploy", "create_app")

fun appsLinkVisible(servers: List<ChatToolServer>): Boolean = servers.any { it.kind == CoolifyKind }

fun appStatusWord(status: String): String = when (status) {
    "running" -> "Running"
    "deploying" -> "Deploying"
    "failed" -> "Failed"
    else -> "Stopped"
}

fun deploymentResultWord(result: String): String = when (result) {
    "deployed" -> "Deployed"
    "failed" -> "Failed"
    "cancelled" -> "Cancelled"
    else -> "Deploying"
}

fun triggeredByWord(triggeredBy: String): String = when (triggeredBy) {
    "agent" -> "An agent"
    "person" -> "You"
    "git" -> "A push to git"
    else -> "Coolify"
}

fun changedByWord(changedBy: String?): String = when (changedBy) {
    "agent" -> "An agent"
    "person" -> "You"
    else -> "Coolify"
}

fun appRunning(status: String): Boolean = status == "running" || status == "deploying"

fun commitLine(deployment: AppDeployment): String = listOfNotNull(deployment.commit?.take(7), deployment.commitMessage).joinToString(" ")

fun appMatches(app: AppSummary, query: String): Boolean {
    val needle = query.trim().lowercase()

    return needle.isEmpty() || listOfNotNull(app.name, app.address, app.repository).any { needle in it.lowercase() }
}

fun filteredLogLines(lines: List<String>, query: String): List<String> {
    val needle = query.trim().lowercase()

    return if (needle.isEmpty()) lines else lines.filter { needle in it.lowercase() }
}

fun envUsedAt(env: AppEnv): String = if (env.buildTime) "Build and runtime" else "Runtime"

fun envShownValue(env: AppEnv, revealed: Boolean): String = if (revealed && !env.secret && env.value != null) env.value else "••••••••••"

fun envNotice(envs: List<AppEnv>): String? {
    val stale = envs.filter { it.stale }.map { it.key }

    return when (stale.size) {
        0 -> null
        1 -> "${stale.first()} changed after the last deploy. Redeploy to use it."
        else -> "${stale.first()} and ${stale.size - 1} more changed after the last deploy. Redeploy to use them."
    }
}

fun envValueProblem(key: String, value: String): String? = if (value.isEmpty()) "Enter a value for $key" else null

fun envNameProblem(key: String, existing: List<AppEnv>): String? = when {
    key.isBlank() -> "Enter a name for the variable"
    !key.trim().matches(Regex("[A-Za-z_][A-Za-z0-9_]*")) -> "Use letters, digits and underscores, and do not start with a digit"
    existing.any { it.key == key.trim() } -> "Enter a different name. ${key.trim()} already exists."
    else -> null
}

fun mostRecentDeployment(deployments: List<AppDeployment>): AppDeployment? = deployments.maxByOrNull { it.startedAt }

data class DeployCall(val deploymentUuid: String, val appName: String?, val address: String?)

fun deployCallOf(tool: String?, output: String?): DeployCall? {
    val name = tool?.substringAfterLast("__") ?: return null

    if (name !in DeployTools || output.isNullOrBlank()) {
        return null
    }

    val parsed = runCatching { Json.parseToJsonElement(output.trim()).jsonObject }.getOrNull() ?: return null
    val uuid = parsed.text("deployment_uuid") ?: return null

    return DeployCall(uuid, parsed.text("app_name"), parsed.text("address"))
}

private fun JsonObject.text(key: String): String? = get(key)?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotBlank() }

fun addressLabel(address: String): String = address.removePrefix("https://").removePrefix("http://").trimEnd('/')

fun addressUrl(address: String): String = if (address.startsWith("http")) address else "https://$address"

fun wildcardLabel(wildcard: String?): String? = wildcard?.removePrefix("https://")?.removePrefix("http://")?.trimEnd('/')?.let { if (it.startsWith("*.")) it else "*.$it" }
