package mycelium

import mycelium.state.Annotation
import mycelium.state.annotationLabel
import mycelium.state.messageWithAnnotations
import kotlin.test.Test
import kotlin.test.assertEquals

class AnnotationsTest {
    @Test
    fun anAttachedAnnotationIsSentAsAQuoteWithItsCommentBeforeTheMessage() {
        val sent = messageWithAnnotations(listOf(Annotation("the retry delay\nfrom RESTORE_BACKOFF_MS", "make this 5000")), "Do it")

        assertEquals("> the retry delay\n> from RESTORE_BACKOFF_MS\n\nmake this 5000\n\nDo it", sent)
    }

    @Test
    fun severalAnnotationsKeepTheirOrderAndAnEmptyCommentSendsTheQuoteAlone() {
        val sent = messageWithAnnotations(listOf(Annotation("one", ""), Annotation("two", "why")), "")

        assertEquals("> one\n\n> two\n\nwhy", sent)
    }

    @Test
    fun withoutAnnotationsTheMessageIsUnchanged() {
        assertEquals("Run the tests", messageWithAnnotations(emptyList(), "  Run the tests "))
    }

    @Test
    fun aChipLabelShowsTheQuoteAndCommentAndShortensALongQuote() {
        assertEquals("\"a b\": fix", annotationLabel(Annotation("a\n  b", "fix")))
        assertEquals("\"${"x".repeat(48)}...\"", annotationLabel(Annotation("x".repeat(60), "")))
    }
}
