package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.InlineTextContent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import mycelium.api.Project

private const val LongestLabel = 32
private val FeedBadgeMarkup = Regex("\\[\\[(project|type|outcome|custom):([^\\[\\]|\\n]+)(?:\\|([a-z0-9-]{1,32}))?]]")
private val BadgeHeight = 18.dp
private val BadgeLineHeight = 20.sp

enum class FeedBadgeKind(val word: String, val spoken: String?) {
    Project("project", "Project"),
    Type("type", "Type"),
    Outcome("outcome", "Outcome"),
    Custom("custom", null),
}

data class FeedBadge(val token: String, val kind: FeedBadgeKind, val label: String, val icon: String? = null) {
    val accessibleName: String get() = kind.spoken?.let { "$it: $label" } ?: label
}

val LocalFeedProjects = staticCompositionLocalOf<Collection<Project>?> { null }

fun parseFeedBadge(text: String, index: Int): FeedBadge? {
    val match = FeedBadgeMarkup.matchAt(text, index) ?: return null
    val kind = FeedBadgeKind.entries.first { it.word == match.groupValues[1] }
    val label = match.groupValues[2].trim()
    val icon = match.groupValues[3].ifEmpty { null }

    if (label.isEmpty() || label.length > LongestLabel || (icon != null && kind != FeedBadgeKind.Custom)) {
        return null
    }

    return FeedBadge(match.value, kind, label, icon)
}

fun feedBadges(text: String): List<FeedBadge> = FeedBadgeMarkup.findAll(text).mapNotNull { parseFeedBadge(text, it.range.first) }.distinct().toList()

fun feedBadgeIcon(badge: FeedBadge): MyIcon = when (badge.kind) {
    FeedBadgeKind.Project -> MyIcon.Folder
    FeedBadgeKind.Type -> when (badge.label.lowercase()) {
        "release" -> MyIcon.Tag
        "feature" -> MyIcon.Lightbulb
        "fix" -> MyIcon.Issue
        "docs" -> MyIcon.BookOpen
        "security" -> MyIcon.Mode
        "chore" -> MyIcon.Settings
        else -> MyIcon.Layers
    }
    FeedBadgeKind.Outcome -> when (badge.label.lowercase()) {
        "live" -> MyIcon.Globe
        "shipped", "passed" -> MyIcon.CircleCheck
        "failed" -> MyIcon.Failed
        "blocked" -> MyIcon.TriangleAlert
        "rolled back" -> MyIcon.ArrowLeft
        else -> MyIcon.Info
    }
    FeedBadgeKind.Custom -> MyIcon.entries.firstOrNull { it.name.equals(badge.icon?.replace("-", ""), ignoreCase = true) } ?: MyIcon.Star
}

@Composable
private fun feedBadgeTint(badge: FeedBadge): Color {
    val colors = MyTheme.colors

    if (badge.kind != FeedBadgeKind.Outcome) return colors.textMuted

    return when (badge.label.lowercase()) {
        "live", "shipped", "passed" -> colors.statusDone
        "failed" -> colors.statusFailed
        "blocked", "rolled back" -> colors.statusAttention
        else -> colors.textMuted
    }
}

@Composable
private fun FeedBadgeChip(badge: FeedBadge, project: Project?) {
    Row(
        Modifier
            .clearAndSetSemantics {
                contentDescription = badge.accessibleName
                role = Role.Image
            }
            .heightIn(min = BadgeHeight)
            .background(MyTheme.colors.bgSelected, ControlShape)
            .padding(horizontal = Tokens.Space.S1_5),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (project != null) {
            ProjectIcon(project)
        } else {
            Icon(feedBadgeIcon(badge), feedBadgeTint(badge), Tokens.Size.IconSm)
        }

        Label(badge.label, style = Tokens.Type.Meta, maxLines = 1, overflow = TextOverflow.Clip)
    }
}

@Composable
internal fun feedBadgeContent(text: String, projects: Collection<Project>): Map<String, InlineTextContent> {
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val style = Tokens.Type.Meta.themed()
    val chromePx = with(density) { Tokens.Space.S1_5.roundToPx() * 2 + Tokens.Size.IconSm.roundToPx() + Tokens.Space.S1.roundToPx() }

    return feedBadges(text).associate { badge ->
        val project = projects.takeIf { badge.kind == FeedBadgeKind.Project }?.firstOrNull { it.name.equals(badge.label, ignoreCase = true) }
        val labelPx = measurer.measure(badge.label, style, maxLines = 1).size.width
        val widthPx = chromePx + labelPx
        val placeholder = with(density) { Placeholder(widthPx.toSp(), BadgeLineHeight, PlaceholderVerticalAlign.TextCenter) }

        badge.token to InlineTextContent(placeholder) {
            Box(contentAlignment = Alignment.CenterStart) {
                FeedBadgeChip(badge, project)
            }
        }
    }
}
