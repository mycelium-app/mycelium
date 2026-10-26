package mycelium

import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.ModelProvider
import mycelium.ui.recentModels
import mycelium.ui.withFavourite
import kotlin.test.Test
import kotlin.test.assertEquals

private fun provider(id: String, models: List<String>, favourites: List<String> = emptyList()) =
    ModelProvider(id = id, kind = "openai_compatible", name = id, models = models, createdAt = "2026-01-01T00:00:00Z", favourites = favourites)

private fun chat(id: String, providerId: String?, model: String?, updatedAt: String) =
    Conversation(id = id, owner = "o", nodeId = "n", agent = "chat", cwd = "", title = id, status = ConversationStatus.Done, model = model, createdAt = updatedAt, updatedAt = updatedAt, providerId = providerId)

class RecentModelsTest {
    @Test
    fun recentModelsAreNewestFirstAndDistinct() {
        val providers = listOf(provider("a", listOf("x", "y")))
        val chats = listOf(chat("1", "a", "x", "2026-01-01"), chat("2", "a", "y", "2026-01-03"), chat("3", "a", "x", "2026-01-02"), chat("4", null, null, "2026-01-04"))

        assertEquals(listOf("y", "x"), recentModels(chats, providers).map { it.second })
    }

    @Test
    fun recentModelsLeaveOutFavouritesAndModelsTheProviderNoLongerLists() {
        val providers = listOf(provider("a", listOf("x", "y"), favourites = listOf("x")))
        val chats = listOf(chat("1", "a", "x", "2026-01-01"), chat("2", "a", "gone", "2026-01-02"), chat("3", "a", "y", "2026-01-03"), chat("4", "removed", "y", "2026-01-04"))

        assertEquals(listOf("y"), recentModels(chats, providers).map { it.second })
    }

    @Test
    fun withFavouriteAddsOnceAndRemoves() {
        val providers = listOf(provider("a", listOf("x")))

        val twice = providers.withFavourite("a", "x", true).withFavourite("a", "x", true)

        assertEquals(listOf("x"), twice[0].favourites)
        assertEquals(emptyList(), twice.withFavourite("a", "x", false)[0].favourites)
    }
}
