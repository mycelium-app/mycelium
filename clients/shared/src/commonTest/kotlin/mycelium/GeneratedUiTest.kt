package mycelium

import mycelium.state.UiAction
import mycelium.state.UiBar
import mycelium.state.UiNode
import mycelium.state.barChartDescription
import mycelium.state.parseGeneratedUi
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class GeneratedUiTest {
    @Test
    fun aColumnOfEveryComponentParses() {
        val node = parseGeneratedUi(
            """{"type":"column","children":[
                {"type":"heading","text":"Restore"},
                {"type":"text","text":"Pick one."},
                {"type":"table","columns":["Plan","Hours"],"rows":[["Full","4.1"],["Incremental"]]},
                {"type":"bar_chart","title":"Hours","bars":[{"label":"Full","value":4.1,"display":"4.1 h"},{"label":"Incremental","value":9}]},
                {"type":"row","children":[{"type":"button","label":"Use full","action":{"type":"send_message","text":"Use full"}}]}
            ]}""",
        )

        val column = node as UiNode.Column

        assertEquals(UiNode.Heading("Restore"), column.children[0])
        assertEquals(UiNode.Text("Pick one."), column.children[1])
        assertEquals(UiNode.Table(listOf("Plan", "Hours"), listOf(listOf("Full", "4.1"), listOf("Incremental", ""))), column.children[2])
        assertEquals(UiNode.BarChart("Hours", listOf(UiBar("Full", 4.1, "4.1 h"), UiBar("Incremental", 9.0, "9"))), column.children[3])
        assertEquals(UiNode.Row(listOf(UiNode.Button("Use full", UiAction.SendMessage("Use full")))), column.children[4])
    }

    @Test
    fun eachButtonActionParses() {
        fun action(json: String) = (parseGeneratedUi("""{"type":"button","label":"Go","action":$json}""") as UiNode.Button).action

        assertEquals(UiAction.SendMessage("hi"), action("""{"type":"send_message","text":"hi"}"""))
        assertEquals(UiAction.SideChat("why"), action("""{"type":"side_chat","text":"why"}"""))
        assertEquals(UiAction.OpenFile("src/main.rs"), action("""{"type":"open_file","path":"src/main.rs"}"""))
    }

    @Test
    fun aBarChartDescribesEachBarInWords() {
        val chart = parseGeneratedUi("""{"type":"bar_chart","title":"Restore time","bars":[{"label":"Full","value":4,"display":"4.1 h"},{"label":"Incremental","value":9,"display":"9.2 h"}]}""") as UiNode.BarChart

        assertEquals("Restore time. Full: 4.1 h. Incremental: 9.2 h", barChartDescription(chart))
    }

    @Test
    fun aProgressBarIsAShareOfItsMaximumAndShowsAPercentageUnlessToldOtherwise() {
        assertEquals(UiNode.Progress("Steps", 0.375f, "3 of 8 steps"), parseGeneratedUi("""{"type":"progress","label":"Steps","value":3,"max":8,"display":"3 of 8 steps"}"""))
        assertEquals(UiNode.Progress("Upload", 0.42f, "42%"), parseGeneratedUi("""{"type":"progress","label":"Upload","value":42}"""))
        assertEquals(UiNode.Progress("Over", 1f, "100%"), parseGeneratedUi("""{"type":"progress","label":"Over","value":12,"max":10}"""))
        assertNull(parseGeneratedUi("""{"type":"progress","label":"Broken","value":1,"max":0}"""))
        assertNull(parseGeneratedUi("""{"type":"progress","label":"Words","value":"half"}"""))
    }

    @Test
    fun invalidShapesAreRejected() {
        assertNull(parseGeneratedUi("not json"))
        assertNull(parseGeneratedUi("[]"))
        assertNull(parseGeneratedUi("""{"type":"carousel"}"""))
        assertNull(parseGeneratedUi("""{"type":"column"}"""))
        assertNull(parseGeneratedUi("""{"type":"column","children":[{"type":"text"}]}"""))
        assertNull(parseGeneratedUi("""{"type":"bar_chart","bars":[{"label":"a","value":"high"}]}"""))
        assertNull(parseGeneratedUi("""{"type":"bar_chart","bars":[]}"""))
        assertNull(parseGeneratedUi("""{"type":"button","label":"Go","action":{"type":"run","command":"rm"}}"""))
        assertNull(parseGeneratedUi("""{"type":"button","label":"Go","action":{"type":"open_file"}}"""))
    }

    @Test
    fun deeplyNestedTreesAreRejected() {
        val deep = (1..20).fold("""{"type":"text","text":"x"}""") { inner, _ -> """{"type":"column","children":[$inner]}""" }

        assertNull(parseGeneratedUi(deep))
    }
}
