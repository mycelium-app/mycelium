package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.runBlocking
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.ui.LocalPlatform
import mycelium.ui.LocalRemoteImages
import mycelium.ui.LocalTouch
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private class ImageTestPlatform : Platform by QuietPlatform {
    val opened = CopyOnWriteArrayList<String>()

    override fun openUrl(url: String) {
        opened += url
    }
}

private const val Trusted = "https://shop.example.com/img/rail-hook.png"
private const val Untrusted = "https://hardware.example.com/track.png"
private const val UntrustedAlt = "Aluminium ceiling track with two sliding hooks"
private const val StripsAlt = "Adhesive strips"

private val FourImages = listOf("Adhesive strips" to "images.example.org", "D-ring and sawtooth kit" to "images.example.org", "Picture rail hook" to "images.example.net", "Gallery cable system" to "images.example.net")
    .mapIndexed { number, (alt, host) -> Triple(alt, host, "https://$host/img/option-$number.png") }

private fun SemanticsNodeInteraction.save(name: String) {
    val png = org.jetbrains.skia.Image.makeFromBitmap(captureToImage().asSkiaBitmap()).encodeToData(org.jetbrains.skia.EncodedImageFormat.PNG)!!.bytes
    val out = java.io.File(System.getenv("MYCELIUM_SHOTS") ?: "build/screens", "$name.png")

    out.parentFile.mkdirs()
    out.writeBytes(png)
}

class RemoteImagesTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val platform = ImageTestPlatform()
    private val gets = CopyOnWriteArrayList<String>()
    private val posts = CopyOnWriteArrayList<String>()
    private val refusals = mutableMapOf<String, Pair<String, String>>()
    private val held = mutableMapOf<String, CompletableDeferred<Unit>>()
    private val json = headersOf(HttpHeaders.ContentType, "application/json")

    private fun needsClick(host: String) = """{"error":"image_needs_click","message":"This image is not from an image search.","host":"$host"}"""

    private fun store(trusted: Set<String>, clicked: Set<String> = emptySet()): Store {
        val engine = MockEngine { request ->
            val url = request.url.parameters["url"]

            when (request.method) {
                HttpMethod.Get -> {
                    gets += url!!
                    held[url]?.await()

                    val refusal = refusals[url]

                    when {
                        refusal != null -> respond("""{"error":"${refusal.first}","message":"${refusal.second}","host":"${java.net.URI(url).host}"}""", HttpStatusCode.UnprocessableEntity, json)
                        url in trusted || url in clicked -> respond(testPng(320, 320), HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "image/png"))
                        else -> respond(needsClick(java.net.URI(url).host), HttpStatusCode.Forbidden, json)
                    }
                }
                else -> {
                    val body = String(request.body.toByteArray())

                    posts += body
                    respond(testPng(320, 320, 0x3b5bdb), HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "image/png"))
                }
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    private fun show(text: String, store: Store, touch: Boolean = false, runsOnModel: Boolean = false, width: Int = 760) {
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalPlatform provides platform, LocalTouch provides touch, LocalRemoteImages provides store.remoteImages("c1")) {
                    Box(Modifier.size(width.dp, 640.dp).background(MyTheme.colors.bgContent)) {
                        ThreadView(listOf(ThreadItem.AgentMessage("m1", text)), onAnswer = { _, _ -> }, onStopWorkflow = {}, runsOnModel = runsOnModel)
                    }
                }
            }
        }
    }

    private fun tile(alt: String, host: String) = rule.onNodeWithContentDescription("$alt, from $host. Open larger")

    private fun loadButton(alt: String, host: String) = rule.onNodeWithContentDescription("Load image from $host: $alt")

    private val gallery = FourImages.joinToString("\n") { (alt, _, url) -> "![$alt]($url)" }

    @Test
    fun aReplyWithOneTrustedImageShowsATileNamedAfterItsAltAndHost() {
        show("This is the one I mean:\n\n![Brass picture rail hook](${Trusted})", store(setOf(Trusted)))

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Brass picture rail hook, from shop.example.com. Open larger").fetchSemanticsNodes().isNotEmpty() }
        tile("Brass picture rail hook", "shop.example.com").assertIsDisplayed()
        assertEquals(listOf(Trusted), gets.toList())
        assertEquals(emptyList(), posts.toList())
    }

    @Test
    fun imagesOnConsecutiveLinesFormOneGalleryOfNamedTiles() {
        show("Four options:\n\n$gallery", store(FourImages.map { it.third }.toSet()))

        rule.waitUntil(5_000) { rule.onAllNodes(hasContentDescription("Open larger", substring = true)).fetchSemanticsNodes().size == 4 }
        rule.onAllNodesWithContentDescription("4 images").assertCountEquals(1)

        for ((alt, host, _) in FourImages) {
            tile(alt, host).assertIsDisplayed()
        }
    }

    @Test
    fun anUntrustedImageShowsAPlaceholderAndSendsNothingToLoadUntilTheButtonIsPressed() {
        show("The track itself:\n\n![$UntrustedAlt]($Untrusted)", store(emptySet()))

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Load image from hardware.example.com: $UntrustedAlt").fetchSemanticsNodes().isNotEmpty() }
        loadButton(UntrustedAlt, "hardware.example.com").assertIsDisplayed()
        rule.onNodeWithText("Not from an image search, so it loads only when you ask.").assertIsDisplayed()
        assertEquals(emptyList(), posts.toList())
    }

    @Test
    fun pressingLoadImageSendsExactlyOnePostForThatUrlAndShowsTheImageInPlace() {
        show("The track itself:\n\n![$UntrustedAlt]($Untrusted)", store(emptySet()))

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Load image from hardware.example.com: $UntrustedAlt").fetchSemanticsNodes().isNotEmpty() }
        loadButton(UntrustedAlt, "hardware.example.com").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("$UntrustedAlt, from hardware.example.com. Open larger").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(listOf("""{"url":"$Untrusted"}"""), posts.toList())
        assertEquals(0, rule.onAllNodesWithContentDescription("Load image from hardware.example.com: $UntrustedAlt").fetchSemanticsNodes().size)
    }

    private fun refusedWith(code: String, message: String) {
        refusals[Trusted] = code to message
        show("Fitting guide:\n\n![Ceiling track end cap]($Trusted)", store(emptySet()))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Not shown: $message").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Not shown: $message").assertIsDisplayed()
        rule.onNodeWithText("Open link").performClick()
        assertEquals(listOf(Trusted), platform.opened.toList())
    }

    @Test
    fun aPrivateAddressRefusalShowsItsMessageAndOpenLink() = refusedWith("image_private_address", "This address is on a private network, which Mycelium does not fetch for agents.")

    @Test
    fun anAddressRefusalShowsItsMessageAndOpenLink() = refusedWith("image_address_refused", "Mycelium would not connect to that address.")

    @Test
    fun aNonHttpsRefusalShowsItsMessageAndOpenLink() = refusedWith("image_not_https", "Only https images are shown.")

    @Test
    fun anOversizedImageRefusalShowsItsMessageAndOpenLink() = refusedWith("image_too_large", "The image is 24 MB, over the 10 MB limit.")

    @Test
    fun aWebPageRefusalShowsItsMessageAndOpenLink() = refusedWith("image_not_image", "images.example.org sent a web page, not an image.")

    @Test
    fun anUnreachableHostRefusalShowsItsMessageAndOpenLink() = refusedWith("image_unavailable", "images.example.org did not answer.")

    @Test
    fun anImageWithNoAltTextStaysALinkAndNothingIsRequested() {
        show("Fitting guide: ![]($Trusted)", store(setOf(Trusted)))

        rule.onNodeWithText(Trusted, substring = true).assertIsDisplayed()
        assertEquals(emptyList(), gets.toList())
    }

    @Test
    fun theViewerOpensOnEnterStepsWithTheArrowKeysAndEscapeReturnsFocusToTheTile() {
        show(gallery, store(FourImages.map { it.third }.toSet()))
        rule.waitUntil(5_000) { rule.onAllNodes(hasContentDescription("Open larger", substring = true)).fetchSemanticsNodes().size == 4 }

        tile("Adhesive strips", "images.example.org").requestFocus().performKeyInput { pressKey(Key.Enter) }
        rule.onNodeWithContentDescription("Image: Adhesive strips").assertIsDisplayed()
        rule.onNodeWithText("1 of 4").assertIsDisplayed()

        rule.onNodeWithContentDescription("Image: Adhesive strips").performKeyInput { pressKey(Key.DirectionRight) }
        rule.onNodeWithText("2 of 4").assertIsDisplayed()
        rule.onNodeWithContentDescription("Image: D-ring and sawtooth kit").performKeyInput { pressKey(Key.DirectionLeft) }
        rule.onNodeWithText("1 of 4").assertIsDisplayed()

        rule.onNodeWithContentDescription("Image: Adhesive strips").performKeyInput { pressKey(Key.Escape) }
        assertEquals(0, rule.onAllNodesWithContentDescription("Image: Adhesive strips").fetchSemanticsNodes().size)
        tile("Adhesive strips", "images.example.org").assertIsFocused()
    }

    @Test
    fun theViewerCaptionNamesTheHostAndOpenPageOpensTheSourcePage() {
        val page = "https://shop.example.com/products/rail-hook"

        show("![Brass picture rail hook]($Trusted \"$page\")", store(setOf(Trusted)))
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Brass picture rail hook, from shop.example.com. Open larger").fetchSemanticsNodes().isNotEmpty() }
        tile("Brass picture rail hook", "shop.example.com").performClick()
        rule.onNodeWithText("Open page").performClick()

        assertEquals(listOf(page), platform.opened.toList())
    }

    @Test
    fun aChatModelReplyShowsTheSameImageTile() {
        show("![Brass picture rail hook]($Trusted)", store(setOf(Trusted)), runsOnModel = true)

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Brass picture rail hook, from shop.example.com. Open larger").fetchSemanticsNodes().isNotEmpty() }
        tile("Brass picture rail hook", "shop.example.com").assertIsDisplayed()
    }

    private fun shot(name: String) {
        rule.waitForIdle()
        rule.onAllNodes(isRoot()).onLast().save(name)
    }

    private fun settled(count: Int) {
        rule.waitUntil(5_000) { rule.onAllNodes(hasContentDescription("Open larger", substring = true)).fetchSemanticsNodes().size == count }
    }

    @Test
    fun screenshot340ReplyWithAnInlineImage() {
        show("Your hallway has a picture rail, so the hook is the option that never touches the plaster. This is the one I mean:\n\n![Brass picture rail hook holding a framed print by a cord]($Trusted \"https://shop.example.com/rail-hook\")\n\nIt takes a cord or chain down to the frame. I can show the other options too.", store(setOf(Trusted)), width = 760)
        settled(1)
        shot("3.40")
    }

    @Test
    fun screenshot341ReplyWithSeveralImages() {
        show("Here are four ways to hang prints on old plaster without drilling, lightest first:\n\n1. **Adhesive strips** hold up to about 7 kg a pair on painted plaster that is sound.\n2. **D-ring and sawtooth kit** still needs a pin, but a fine one.\n3. **Picture rail hook** if the hallway has a rail near the ceiling.\n4. **Gallery cable system** hangs from a ceiling track.\n\n$gallery\n\nIf you tell me the frame weighs I will say which of these holds them.", store(FourImages.map { it.third }.toSet()), width = 760)
        settled(4)
        tile("D-ring and sawtooth kit", "images.example.org").requestFocus()
        shot("3.41")
    }

    @Test
    fun screenshot342ImageEnlarged() {
        show("![D-ring and sawtooth hanger kit with screws](${FourImages[1].third} \"https://shop.example.com/d-ring\")\n![Picture rail hook](${FourImages[2].third})", store(FourImages.map { it.third }.toSet()), width = 760)
        settled(2)
        tile("D-ring and sawtooth hanger kit with screws", "images.example.org").performClick()
        rule.onNodeWithText("Open page").assertIsDisplayed()
        shot("3.42")
    }

    @Test
    fun screenshot343PlaceholdersLoadingAndRefusals() {
        val loading = "https://images.example.org/img/loading.png"
        val private = "https://intranet.example.org/img/hangers.png"
        val large = "https://images.example.org/img/cable.png"
        val page = "https://images.example.org/img/end-cap.png"

        held[loading] = CompletableDeferred()
        refusals[private] = "image_private_address" to "This address is on a private network, which Mycelium does not fetch for agents."
        refusals[large] = "image_too_large" to "The image is 24 MB, over the 10 MB limit."
        refusals[page] = "image_not_image" to "images.example.org sent a web page, not an image."

        show("Close-ups of the fixings:\n\n![Adhesive strips](${Trusted})\n![D-ring hangers]($loading)\n![Rail hook, close up]($private)\n![Cable system, wide shot]($large)\n\nThe track itself, from the supplier page I read:\n\n![$UntrustedAlt]($Untrusted)\n\nAnd the part that finishes the cable track:\n\n![Ceiling track end cap]($page)\n\nFitting guide: ![](https://shop.example.com/img/rail-hook-guide.png)", store(setOf(Trusted)), width = 760)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Not shown: images.example.org sent a web page, not an image.").fetchSemanticsNodes().isNotEmpty() }
        shot("3.43")
    }

    @Test
    fun screenshot344AChatModelReply() {
        show("Close-ups of the fixings:\n\n![Brass picture rail hook holding a framed print by a cord]($Trusted)", store(setOf(Trusted)), runsOnModel = true, width = 760)
        settled(1)
        shot("3.44")
    }

    @Test
    fun screenshot345PhoneGalleryPlaceholderAndEnlarged() {
        show("The four common no-drill options:\n\n$gallery\n\nAnd the end cap, from the supplier page:\n\n![Ceiling track end cap, white]($Untrusted)", store(FourImages.map { it.third }.toSet()), touch = true, width = 390)
        settled(4)
        rule.onNodeWithContentDescription("Load image from hardware.example.com: Ceiling track end cap, white").assertIsDisplayed()
        shot("3.45")
        tile("D-ring and sawtooth kit", "images.example.org").performClick()
        shot("3.45-enlarged")
    }
}
