package mycelium.ui

import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.Forge

private const val AllCategories = "All"
private val TileMinWidth = 280.dp

internal fun forgeProduct(forge: Forge): String = if (forge.kind == "gitlab") "GitLab" else "GitHub"

data class CatalogueTile(val name: String, val category: IntegrationCategory, val agentsCan: String, val asksFirst: String, val brand: String?, val detail: String?, val footer: String, val added: Boolean, val icon: MyIcon, val onChoose: () -> Unit)

@Composable
private fun Tile(tile: CatalogueTile, modifier: Modifier) {
    val colors = MyTheme.colors

    Column(
        modifier
            .border(Tokens.Size.Hairline, colors.border, BlockShape)
            .pressable(tile.onChoose, role = Role.Button, shape = BlockShape, base = colors.bgPanel)
            .padding(Tokens.Space.S3),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Label(tile.name, style = Tokens.Type.UiStrong, maxLines = 1)
                Label(listOfNotNull(tile.category.label, tile.detail).joinToString(" · "), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            val brand = tile.brand

            if (brand != null) {
                BrandLogo(brand, Tokens.Size.IconMd * 1.5f)
            } else {
                Icon(tile.icon, colors.textMuted, Tokens.Size.IconMd * 1.5f)
            }
        }

        Label("Agents can: ${tile.agentsCan}", style = Tokens.Type.Meta, color = colors.textMuted)
        Label("Asks first: ${tile.asksFirst}", style = Tokens.Type.Meta, color = colors.textMuted)
        Hairline()
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
            if (tile.added) {
                Icon(MyIcon.Check, colors.statusDone, Tokens.Size.IconSm)
            }

            Label(tile.footer, style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
        }
    }
}

@Composable
private fun TileGrid(tiles: List<CatalogueTile>) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val columns = (maxWidth / TileMinWidth).toInt().coerceIn(1, 3)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            for (row in tiles.chunked(columns)) {
                Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    for (tile in row) {
                        Tile(tile, Modifier.weight(1f).fillMaxHeight())
                    }

                    repeat(columns - row.size) {
                        Spacer(Modifier.weight(1f))
                    }
                }
            }
        }
    }
}

@Composable
fun IntegrationsCatalogue(integrations: List<Integration>, forges: List<Forge>, onBack: () -> Unit, onChoose: (CatalogueEntry) -> Unit, onOpen: (Integration) -> Unit, onAddForge: (Forge) -> Unit) {
    val colors = MyTheme.colors
    var query by remember { mutableStateOf("") }
    var category by remember { mutableStateOf(AllCategories) }

    fun inCategory(wanted: IntegrationCategory) = category == AllCategories || category == wanted.label

    val builtIn = BuiltInCatalogue.filter { matchesCatalogue(it, query) && inCategory(it.category) }.map { entry ->
        val existing = integrations.find { it.key == entry.key }
        val footer = when {
            existing == null -> "Not added"
            existing.accounts.size == 1 -> "Added"
            else -> "Added, ${existing.accounts.size} accounts"
        }

        CatalogueTile(entry.name, entry.category, entry.agentsCan, entry.asksFirst, entry.brand, entry.detail, footer, existing != null, entry.icon) {
            if (existing != null) onOpen(existing) else onChoose(entry)
        }
    }
    val forgeTiles = forges.filter { it.kind == "gitlab" || it.kind == "github" }.mapNotNull { forge ->
        val name = forgeProduct(forge)
        val entry = CatalogueEntry("forge:${forge.id}", name, "mcp", IntegrationCategory.Developer, "Read issues and merge requests, comment and open them after asking.", "Anything that changes a project.", listOf("issues", "merge_requests", "pipelines"), forge.kind)

        if (!matchesCatalogue(entry, query) || !inCategory(entry.category)) {
            return@mapNotNull null
        }

        val existing = integrations.find { integration -> integration.accounts.any { it.forgeId == forge.id } }
        val footer = if (existing != null) "Added from ${forge.baseUrl.removePrefix("https://")}" else "Uses your ${forge.baseUrl.removePrefix("https://")} forge token"

        CatalogueTile(name, entry.category, entry.agentsCan, entry.asksFirst, entry.brand, entry.detail, footer, existing != null, MyIcon.Forge) {
            if (existing != null) onOpen(existing) else onAddForge(forge)
        }
    }
    val showMcp = matchesCatalogue(AnyMcpEntry, query) && inCategory(AnyMcpEntry.category)

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            BackButton("Integrations", onBack, showText = true)
        }

        Label("Add integration", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        ListSearch(query, { query = it }, "services and tools")
        FilterChips(listOf(AllCategories) + IntegrationCategory.entries.map { it.label }, category, { category = it }, "Category")

        if (showMcp) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .border(Tokens.Size.Hairline, colors.border, BlockShape)
                    .pressable({ onChoose(AnyMcpEntry) }, role = Role.Button, shape = BlockShape, base = colors.bgPanel)
                    .padding(Tokens.Space.S3),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(MyIcon.Server, colors.textMuted, Tokens.Size.IconMd * 1.5f)

                Column(Modifier.weight(1f)) {
                    Label(AnyMcpEntry.name, style = Tokens.Type.UiStrong, maxLines = 1)
                    Label(AnyMcpEntry.agentsCan, style = Tokens.Type.Meta, color = colors.textMuted)
                }
            }
        }

        if (builtIn.isNotEmpty()) {
            Label("Built in", Modifier.semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)
            TileGrid(builtIn)
        }

        if (forgeTiles.isNotEmpty()) {
            Label("From your forges", Modifier.semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)
            TileGrid(forgeTiles)
        }

        if (!showMcp && builtIn.isEmpty() && forgeTiles.isEmpty()) {
            Label("Nothing matches. Try a service, a category or a tool name.", color = colors.textMuted)
        }
    }
}
