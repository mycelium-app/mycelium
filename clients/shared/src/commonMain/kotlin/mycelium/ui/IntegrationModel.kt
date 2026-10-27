package mycelium.ui

import kotlinx.serialization.json.jsonPrimitive
import mycelium.api.ChatToolServer
import mycelium.state.instantOrNull
import kotlin.time.Clock
import kotlin.time.Duration.Companion.days
import kotlin.time.Instant

enum class IntegrationCategory(val label: String) {
    Chat("Chat"),
    Mail("Mail"),
    Calendar("Calendar"),
    Docs("Docs"),
    Media("Media"),
    Finance("Finance"),
    Developer("Developer"),
    AiGateway("AI gateway"),
}

enum class AccountState(val attention: Boolean) {
    NeedsSignIn(true),
    Expired(true),
    Error(true),
    ExpiresSoon(true),
    Connected(false),
    Disabled(false),
}

enum class IntegrationGroup(val title: String) {
    NeedsAttention("Needs attention"),
    Connected("Connected"),
    Disabled("Disabled"),
}

data class CatalogueEntry(
    val key: String,
    val name: String,
    val kind: String,
    val category: IntegrationCategory,
    val agentsCan: String,
    val asksFirst: String,
    val tools: List<String>,
    val brand: String? = null,
    val detail: String? = null,
    val icon: MyIcon = MyIcon.Server,
)

val BuiltInCatalogue = listOf(
    CatalogueEntry("email", "Email", "email", IntegrationCategory.Mail, "Search, read and move mail.", "Sending mail asks first.", listOf("list_folders", "search_messages", "read_message", "move_message", "send_message")),
    CatalogueEntry("caldav", "Calendar", "caldav", IntegrationCategory.Calendar, "List calendars, and list and search events in a date range.", "Creating, changing and deleting events ask first.", listOf("list_calendars", "list_events", "search_events", "create_event", "update_event", "delete_event"), detail = "CalDAV", icon = MyIcon.Calendar),
    CatalogueEntry("zulip", "Zulip", "zulip", IntegrationCategory.Chat, "Read and search channels and send messages.", "Every call asks first and you can edit the response.", listOf("list_channels", "get_messages", "search_messages", "send_message"), "zulip"),
    CatalogueEntry("stoat", "Stoat", "stoat", IntegrationCategory.Chat, "Read and search servers your bot is in.", "Sending messages asks first.", listOf("list_servers", "get_messages", "search_messages", "send_message"), "stoat"),
    CatalogueEntry("discord", "Discord", "discord", IntegrationCategory.Chat, "Read and search the servers your bot is in and send messages.", "Sending messages asks first, and each one says the Mycelium agent posted it.", listOf("list_servers", "list_channels", "get_messages", "search_messages", "send_message"), "discord"),
    CatalogueEntry("outline", "Outline", "outline", IntegrationCategory.Docs, "Search, read and write documents, collections and comments.", "Writing asks first.", listOf("search", "fetch", "create_document", "update_document", "list_collections", "create_comment")),
    CatalogueEntry("lastfm", "Last.fm", "lastfm", IntegrationCategory.Media, "Read your listening history and top artists. Sign in to love tracks and scrobble.", "Loving tracks and scrobbling ask first.", listOf("get_recent_tracks", "get_top_artists", "love_track", "unlove_track", "scrobble", "update_now_playing"), "lastfm"),
    CatalogueEntry("paperless", "Paperless-ngx", "paperless", IntegrationCategory.Docs, "Search documents, read their text and list tags, correspondents and document types.", "Changing a document's tags or correspondent asks first.", listOf("search_documents", "get_document", "list_tags", "list_correspondents", "list_document_types", "update_document"), "paperless"),
    CatalogueEntry("freeagent", "FreeAgent", "freeagent", IntegrationCategory.Finance, "Read contacts, invoices, bills, bank transactions and expenses.", "Creating a draft invoice or an expense asks first.", listOf("list_contacts", "list_invoices", "list_bills", "list_bank_transactions", "list_expenses", "create_draft_invoice", "create_expense"), icon = MyIcon.Receipt),
    CatalogueEntry("actual", "Actual Budget", "actual", IntegrationCategory.Finance, "Read accounts, categories, transactions in a range and a month's budget. Needs an Actual HTTP API.", "Adding a transaction asks first.", listOf("list_accounts", "list_categories", "get_transactions", "get_budget_month", "add_transaction"), "actual"),
    CatalogueEntry("litellm", "LiteLLM", "litellm", IntegrationCategory.AiGateway, "Use the MCP tools your gateway publishes, with your gateway key.", "Tools that change things ask first.", listOf("mcp")),
)

val AnyMcpEntry = CatalogueEntry("mcp", "Any MCP server", "mcp", IntegrationCategory.Developer, "Connect a server that speaks MCP over HTTP, by its address, with a token or by signing in. Agents get whatever tools it offers.", "Tools that change things ask first.", listOf("mcp"))

data class Integration(val key: String, val name: String, val kind: String, val category: IntegrationCategory, val accounts: List<ChatToolServer>, val brand: String?) {
    val states: List<AccountState> get() = accounts.map(::accountState)

    val group: IntegrationGroup
        get() = when {
            states.any { it.attention } -> IntegrationGroup.NeedsAttention
            states.all { it == AccountState.Disabled } -> IntegrationGroup.Disabled
            else -> IntegrationGroup.Connected
        }

    val worst: AccountState get() = states.minByOrNull { it.ordinal } ?: AccountState.Connected

    val tools: List<String> get() = accounts.flatMap { server -> server.tools.map { it.name } }.distinct()

    val lastUsedAt: String? get() = accounts.mapNotNull { it.lastUsedAt }.maxOrNull()
}

fun accountState(server: ChatToolServer): AccountState = when {
    !server.enabled -> AccountState.Disabled
    server.status == "expired" -> AccountState.Expired
    server.status == "needs_sign_in" -> AccountState.NeedsSignIn
    server.status == "expires_soon" -> AccountState.ExpiresSoon
    server.lastError != null -> AccountState.Error
    else -> AccountState.Connected
}

private fun catalogueKey(server: ChatToolServer): String? = when {
    server.forgeId != null -> null
    server.settings?.get("app")?.jsonPrimitive?.content == "outline" -> "outline"
    server.kind in setOf("email", "caldav", "zulip", "stoat", "lastfm", "litellm", "discord", "paperless", "freeagent", "actual") -> server.kind
    else -> null
}

fun forgeBrand(server: ChatToolServer): String? = when {
    server.forgeId == null -> null
    "githubcopilot" in server.url || "github" in server.name.lowercase() -> "github"
    else -> "gitlab"
}

fun integrationsOf(servers: List<ChatToolServer>): List<Integration> {
    val grouped = servers.groupBy { catalogueKey(it) ?: "server:${it.id}" }

    return grouped.map { (key, accounts) ->
        val entry = BuiltInCatalogue.find { it.key == key }
        val first = accounts.first()

        if (entry != null) {
            Integration(key, entry.name, entry.kind, entry.category, accounts, entry.brand)
        } else {
            Integration(key, first.name, first.kind, IntegrationCategory.Developer, accounts, forgeBrand(first))
        }
    }.sortedBy { it.name.lowercase() }
}

fun entryFor(integration: Integration): CatalogueEntry? = BuiltInCatalogue.find { it.key == integration.key }

fun matchesSearch(integration: Integration, query: String): Boolean {
    val needle = query.trim().lowercase()

    if (needle.isEmpty()) {
        return true
    }

    val haystack = buildList {
        add(integration.name)
        add(integration.category.label)
        integration.accounts.forEach { server ->
            add(server.name)
            add(server.url)
            server.account?.let { add(it) }
        }
        addAll(integration.tools)
    }

    return haystack.any { needle in it.lowercase() }
}

fun matchesCatalogue(entry: CatalogueEntry, query: String): Boolean {
    val needle = query.trim().lowercase()

    return needle.isEmpty() || (listOf(entry.name, entry.category.label, entry.agentsCan) + entry.tools).any { needle in it.lowercase() }
}

fun accountTitle(server: ChatToolServer): String = server.account ?: server.url.removePrefix("https://").removePrefix("http://").trimEnd('/').ifBlank { server.name }

fun accountsSummary(integration: Integration): String {
    val accounts = integration.accounts

    return if (accounts.size == 1) {
        accountTitle(accounts.first())
    } else {
        "${accounts.size} accounts: ${accounts.joinToString(", ") { accountTitle(it) }}"
    }
}

private val Months = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")

fun expiryDate(iso: String): String? {
    val date = iso.take(10).split("-").takeIf { it.size == 3 } ?: return null
    val month = date[1].toIntOrNull()?.let { Months.getOrNull(it - 1) } ?: return null
    val day = date[2].toIntOrNull() ?: return null

    return "$day $month ${date[0]}"
}

fun expiryNote(server: ChatToolServer): String? {
    val date = server.tokenExpiresAt?.let(::expiryDate) ?: return null

    return when (server.status) {
        "expired" -> "Expired $date. Replace the token to reconnect."
        "expires_soon" -> "Expires $date. Replace the token before then."
        else -> "Expires $date."
    }
}

fun daysUntil(iso: String, now: Instant = Clock.System.now()): Int? {
    val at = instantOrNull(iso) ?: return null

    return (at - now).inWholeDays.toInt()
}

fun statusWords(integration: Integration): String {
    val states = integration.states
    val worst = integration.worst
    val several = states.size > 1
    val count = states.count { it == worst }

    return when (worst) {
        AccountState.Connected -> if (integration.kind == "lastfm" && integration.accounts.none { it.signedIn }) "Connected, read only" else "Connected"
        AccountState.Disabled -> "Disabled: agents cannot use it"
        AccountState.NeedsSignIn -> if (several) "$count of ${states.size} needs sign-in" else "Needs sign-in"
        AccountState.Expired -> if (several) "$count of ${states.size} token expired" else "Token expired"
        AccountState.ExpiresSoon -> {
            val soonest = integration.accounts.filter { accountState(it) == AccountState.ExpiresSoon }.mapNotNull { it.tokenExpiresAt }.minOrNull()
            val date = soonest?.let(::expiryDate)

            if (several) "$count of ${states.size} token expires soon" else if (date != null) "Token expires soon, on $date" else "Token expires soon"
        }
        AccountState.Error -> {
            val message = integration.accounts.firstOrNull { accountState(it) == AccountState.Error }?.lastError

            if (several) "$count of ${states.size} has an error" else "Error: ${message ?: "the last call failed"}"
        }
    }
}

fun accountStatusWords(server: ChatToolServer): String = when (accountState(server)) {
    AccountState.Connected -> "Connected"
    AccountState.Disabled -> "Disabled"
    AccountState.NeedsSignIn -> "Needs sign-in"
    AccountState.Expired -> "Token expired"
    AccountState.ExpiresSoon -> "Expires soon"
    AccountState.Error -> "Error: ${server.lastError ?: "the last call failed"}"
}

val ExpiryPresets = listOf(7, 30, 60, 90)

fun expiryIn(days: Int, now: Instant = Clock.System.now()): String = (now + days.days).toString()

fun expiryOn(date: String): String? {
    val trimmed = date.trim()

    return instantOrNull("${trimmed}T00:00:00Z")?.toString()
}
