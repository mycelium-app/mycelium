package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.Goal
import mycelium.api.EventKinds
import mycelium.api.Event
import mycelium.api.GoalDelivery
import mycelium.api.GoalState
import mycelium.api.GoalWait
import mycelium.state.ThreadItem
import mycelium.state.buildThread
import kotlinx.serialization.json.Json
import mycelium.api.MyceliumApi
import mycelium.api.SetGoalRequest
import mycelium.api.StreamMessage
import mycelium.api.WalkthroughFile
import mycelium.api.parseStreamMessage
import mycelium.state.AppState
import mycelium.state.DiffLine
import mycelium.state.goalMessage
import mycelium.state.goalProgress
import mycelium.state.goalCanNudgeNow
import mycelium.state.parseRepeatLimit
import mycelium.state.parseTimeLimit
import mycelium.state.parseTokenBudget
import mycelium.state.parseWindowLimit
import mycelium.state.reduce
import mycelium.state.stepPosition
import mycelium.state.walkthroughDiff
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.time.Instant

private fun goal(state: GoalState, reason: String? = null) = Goal("c1", "Every test passes", 5_000_000, 2_100_000, 7, state, reason)

private const val GoalJson = """{"type":"goal","conversation_id":"c1","text":"Every test passes","token_budget":5000000,"tokens_used":2100000,"turns":7,"state":"active"}"""

private const val WalkthroughJson = """{"type":"walkthrough","id":"w1","conversation_id":"c1","created_at":"2026-10-23T10:00:00Z","steps":[{"title":"Add strings","key":false,"explanation":"New file.","files":[{"path":"a.ts","diff":"+a"}]},{"title":"Use strings","key":true,"explanation":"","files":[]}]}"""

private val Now = Instant.parse("2026-10-23T14:00:00Z")

private fun limited(timeLimitSeconds: Long? = null, fiveHourLimit: Int? = null, fiveHourUsed: Double? = null, weeklyLimit: Int? = null, weeklyUsed: Double? = null, tokenBudget: Long? = 5_000_000) =
    Goal("c1", "Every test passes", tokenBudget, 2_100_000, 7, GoalState.Active, null, timeLimitSeconds, fiveHourLimit, weeklyLimit, "2026-10-23T12:40:00Z", fiveHourUsed, weeklyUsed)

class GoalsTest {
    @Test
    fun aRunningFirstTurnIsNumberedOneAndMissingUsageIsShownAsPending() {
        val active = Goal("c1", "Build it", state = GoalState.Active, turnActive = true, tokensKnown = false)
        assertEquals("Turn 1  ·  Token usage pending", goalProgress(active))
        assertEquals("Turn 1  ·  300 tokens", goalProgress(active.copy(tokensKnown = true, tokensUsed = 300)))
    }

    @Test
    fun theBudgetIsReadAsAPlainNumberOrWithAKMOrBSuffix() {
        assertEquals(5_000_000L, parseTokenBudget("5000000"))
        assertEquals(5_000_000L, parseTokenBudget("5M"))
        assertEquals(500_000L, parseTokenBudget(" 500k "))
        assertEquals(2_500_000L, parseTokenBudget("2.5m"))
        assertEquals(1_000_000_000L, parseTokenBudget("1b"))
        assertEquals(1_200_000L, parseTokenBudget("1,200,000"))
    }

    @Test
    fun aBudgetThatIsNotAPositiveNumberIsRefused() {
        assertNull(parseTokenBudget(""))
        assertNull(parseTokenBudget("lots"))
        assertNull(parseTokenBudget("0"))
        assertNull(parseTokenBudget("-5k"))
    }

    @Test
    fun theProgressNamesTheTurnAndTheTokensUsedOfTheBudget() {
        val progress = goalProgress(goal(GoalState.Active))

        assertContains(progress, "Turn 7")
        assertContains(progress, "2.1M of 5M")
    }

    @Test
    fun theProgressShowsTheTimeLeftAndTheWindowsThatHaveALimit() {
        val goal = limited(timeLimitSeconds = 9_600, fiveHourLimit = 80, fiveHourUsed = 63.6, weeklyLimit = 90, weeklyUsed = 40.0)

        val progress = goalProgress(goal, Now)

        assertContains(progress, "1h 20m left")
        assertContains(progress, "64%")
        assertContains(progress, "40%")
    }

    @Test
    fun theProgressWithoutABudgetShowsTheTokensUsedAndNoLimitsThatWereNotSet() {
        val progress = goalProgress(limited(tokenBudget = null, fiveHourUsed = 70.0), Now)

        assertContains(progress, "2.1M")
        assertFalse(progress.contains("70%"))
    }

    @Test
    fun aTimeLimitThatHasPassedIsSaidSoAndAWindowWithNoReadingIsUnknown() {
        val goal = limited(timeLimitSeconds = 3_600, fiveHourLimit = 80)

        val progress = goalProgress(goal, Now)

        assertContains(progress, "time limit reached")
        assertContains(progress, "unknown")
    }

    @Test
    fun timeHeldAtAUsageLimitDoesNotCountTowardsTheTimeLimit() {
        val goal = limited(timeLimitSeconds = 3_600)

        assertContains(goalProgress(goal.copy(heldSeconds = 1_800), Now), "10m left")
        assertContains(goalProgress(goal.copy(heldSeconds = 0, heldSince = "2026-10-23T13:20:00Z"), Now), "20m left")
    }

    @Test
    fun aHeldGoalSaysItCarriesOnWhenTheLimitResets() {
        val held = goal(GoalState.Active).copy(heldSince = "2026-10-23T13:30:00Z")

        assertEquals("The goal carries on when the limit resets. Time on hold does not count towards the time limit.", goalMessage(held))
    }

    @Test
    fun aGoalNotYetSentSaysWhenItWillBeAndAFailedOneSaysWhy() {
        val pending = goal(GoalState.Active).copy(delivery = GoalDelivery.Pending, turnActive = true)

        assertEquals("Sends to Claude Code when its current turn ends.", goalMessage(pending, "claude"))
        assertEquals("Sending the goal to Codex.", goalMessage(pending.copy(turnActive = false), "codex"))
        assertEquals("Check that laptop is online.", goalMessage(goal(GoalState.Active).copy(delivery = GoalDelivery.Failed, deliveryReason = "Check that laptop is online.")))
    }

    @Test
    fun aGoalMessageFromMyceliumIsMarkedWithWhenItWasSent() {
        val thread = buildThread(listOf(Event("c1", 1, EventKinds.UserMessage, Json.parseToJsonElement("""{"text":"Work towards this goal: ship","origin":"goal","sent_at":"2026-10-23T14:02:00Z"}"""))))

        assertEquals("2026-10-23T14:02:00Z", (thread.single() as ThreadItem.UserMessage).goalSentAt)
    }

    @Test
    fun aNudgeFromMyceliumCarriesItsNumberAndWhyNow() {
        val thread = buildThread(listOf(Event("c1", 1, EventKinds.UserMessage, Json.parseToJsonElement("""{"text":"Continue towards the goal: ship","origin":"goal","sent_at":"2026-10-23T14:02:00Z","nudge":2,"why":"the turn ended without a report"}"""))))
        val nudge = thread.single() as ThreadItem.UserMessage

        assertEquals(listOf(2, "the turn ended without a report"), listOf(nudge.goalNudge, nudge.goalWhy))
    }

    @Test
    fun theGoalMessageIsNotANudge() {
        val thread = buildThread(listOf(Event("c1", 1, EventKinds.UserMessage, Json.parseToJsonElement("""{"text":"Work towards this goal: ship","origin":"goal","sent_at":"2026-10-23T14:02:00Z","nudge":0,"why":"you set the goal"}"""))))

        assertNull((thread.single() as ThreadItem.UserMessage).goalNudge)
    }

    @Test
    fun aReportFromTheAgentIsATimedCardInTheThread() {
        val thread = buildThread(listOf(Event("c1", 1, EventKinds.GoalReport, Json.parseToJsonElement("""{"outcome":"blocked","reason":"The CI secret is missing.","at":"2026-10-23T14:02:00Z"}"""))))

        assertEquals(ThreadItem.GoalReport("e1", "blocked", "The CI secret is missing.", "2026-10-23T14:02:00Z"), thread.single())
    }

    @Test
    fun aGoalBacksOffAfterTheSameOutcomeAndOffersNudgeNow() {
        val waiting = goal(GoalState.Active).copy(waitReason = GoalWait.RepeatedOutcome, repeats = 2, nextNudgeAt = "2026-10-23T14:02:00Z")

        assertEquals("The last two turns ended the same way, so Mycelium is waiting before it nudges again at 14:02. If the next turn ends the same way too, the goal stops.", goalMessage(waiting) { "14:02" })
        assertTrue(goalCanNudgeNow(waiting))
    }

    @Test
    fun aGoalHeldForAQuestionCannotBeNudged() {
        val held = goal(GoalState.Active).copy(waitReason = GoalWait.QuestionWaiting, heldSince = "2026-10-23T13:30:00Z")

        assertEquals("Held for your answer to Claude Code's question. Mycelium nudges once you answer. Time on hold does not count towards the time limit.", goalMessage(held, "claude"))
        assertFalse(goalCanNudgeNow(held))
    }

    @Test
    fun aGoalWaitingForBackgroundWorkSaysWhenItNudgesAnyway() {
        val waiting = goal(GoalState.Active).copy(waitReason = GoalWait.BackgroundWork, nextNudgeAt = "2026-10-23T14:12:00Z")

        assertEquals("Claude Code is waiting for background work. Mycelium nudges when it reports back, or at 14:12 if it has not.", goalMessage(waiting, "claude") { "14:12" })
        assertTrue(goalCanNudgeNow(waiting))
    }

    @Test
    fun aReportedGoalSaysWhoReportedItAndWhen() {
        val done = goal(GoalState.Met, "All 214 tests pass.").copy(reportedAt = "2026-10-23T14:02:00Z")
        val blocked = goal(GoalState.Stuck, "The CI secret is missing.").copy(reportedAt = "2026-10-23T14:02:00Z")

        assertEquals("Goal met. Claude Code reported it done at 14:02.", goalMessage(done, "claude") { "14:02" })
        assertEquals("Blocked. Claude Code needs you: The CI secret is missing. Continue once it is in place, or clear the goal.", goalMessage(blocked, "claude"))
    }

    @Test
    fun aGoalStoppedForRepeatingItselfOffersResume() {
        val stalled = goal(GoalState.Stalled, "Stopped after 3 turns ended the same way: Two tests still fail")

        assertEquals("Stopped after 3 turns ended the same way: Two tests still fail. Resume to let Codex try again, or send it a message first.", goalMessage(stalled, "codex"))
    }

    @Test
    fun theRepeatLimitIsAWholeNumberOfAtLeastTwo() {
        assertEquals(3, parseRepeatLimit(" 3 "))
        assertNull(parseRepeatLimit("1"))
        assertNull(parseRepeatLimit("three"))
        assertNull(parseRepeatLimit("2.5"))
    }

    @Test
    fun aTimeLimitIsReadAsHoursMinutesAndSeconds() {
        assertEquals(7_200L, parseTimeLimit("2h"))
        assertEquals(2_700L, parseTimeLimit(" 45m "))
        assertEquals(5_400L, parseTimeLimit("1h 30m"))
        assertEquals(5_400L, parseTimeLimit("1.5H"))
        assertEquals(90L, parseTimeLimit("90s"))
    }

    @Test
    fun aTimeLimitThatIsNotAPositiveDurationIsRefused() {
        assertNull(parseTimeLimit(""))
        assertNull(parseTimeLimit("2"))
        assertNull(parseTimeLimit("soon"))
        assertNull(parseTimeLimit("0m"))
        assertNull(parseTimeLimit("2h later"))
    }

    @Test
    fun aWindowLimitIsAPercentFromOneToOneHundredWithOrWithoutTheSign() {
        assertEquals(80, parseWindowLimit("80%"))
        assertEquals(80, parseWindowLimit(" 80 "))
        assertEquals(100, parseWindowLimit("100"))
        assertNull(parseWindowLimit("0"))
        assertNull(parseWindowLimit("101%"))
        assertNull(parseWindowLimit("most"))
    }

    @Test
    fun aGoalThatReachedALimitSaysWhyAndGoalsWithoutLimitsStillDecode() {
        assertEquals("5-hour window at 82%, over your limit of 80%.", goalMessage(limited().copy(state = GoalState.LimitReached, reason = "5-hour window at 82%, over your limit of 80%.")))

        val decoded = parseStreamMessage("""{"type":"goal","conversation_id":"c1","text":"T","tokens_used":0,"turns":0,"started_at":"2026-10-23T12:40:00Z","five_hour_limit":80,"five_hour_used":64.5,"state":"limit_reached"}""")

        assertIs<StreamMessage.GoalChanged>(decoded)
        assertEquals(listOf(null, 80, 64.5, GoalState.LimitReached), decoded.goal.let { listOf(it.tokenBudget, it.fiveHourLimit, it.fiveHourUsed, it.state) })
    }

    @Test
    fun anActiveGoalSaysNothingAndAnEndedOneSaysHowItEnded() {
        assertNull(goalMessage(goal(GoalState.Active)))
        assertEquals("Goal met. All 214 tests pass.", goalMessage(goal(GoalState.Met, "All 214 tests pass.")))
        assertEquals("Blocked. The CI secret is missing.", goalMessage(goal(GoalState.Stuck, "The CI secret is missing.")))
        assertEquals("The goal is not met and the budget is used up.", goalMessage(goal(GoalState.OverBudget, "The goal is not met and the budget is used up.")))
        assertEquals("Paused because you stopped the turn.", goalMessage(goal(GoalState.Paused, "Paused because you stopped the turn.")))
        assertEquals("Paused", goalMessage(goal(GoalState.Paused)))
    }

    @Test
    fun goalMessagesOnTheStreamAreKeptPerConversationAndClearedWhenTheGoalIs() {
        val message = parseStreamMessage(GoalJson)

        assertIs<StreamMessage.GoalChanged>(message)
        val held = reduce(AppState(), message)
        assertEquals(GoalState.Active, held.goals.getValue("c1").state)

        val cleared = reduce(held, parseStreamMessage("""{"type":"goal_cleared","conversation_id":"c1"}""")!!)
        assertEquals(emptyMap(), cleared.goals)
    }

    @Test
    fun aWalkthroughOnTheStreamIsKeptPerConversation() {
        val held = reduce(AppState(), parseStreamMessage(WalkthroughJson)!!)

        assertEquals(listOf("Add strings", "Use strings"), held.walkthroughs.getValue("c1").steps.map { it.title })
        assertEquals(listOf(false, true), held.walkthroughs.getValue("c1").steps.map { it.key })
    }

    @Test
    fun aGoalCheckConversationNeverReachesTheConversationList() {
        val check = """{"type":"conversation","id":"k1","owner":"u","node_id":"n","agent":"claude","cwd":"/w","title":"Goal check","status":"working","created_at":"2026-10-23T10:00:00Z","updated_at":"2026-10-23T10:00:00Z","parent_id":"c1","side_kind":"check"}"""
        val side = check.replace("\"check\"", "\"btw\"").replace("k1", "k2")

        val held = reduce(reduce(AppState(), parseStreamMessage(check)!!), parseStreamMessage(side)!!)

        assertEquals(setOf("k2"), held.conversations.keys)
    }

    @Test
    fun aDiffWithoutHunkHeadersIsShownLineByLineWithoutNumbers() {
        val diff = walkthroughDiff(WalkthroughFile("a.ts", "-const title = \"Restore\"\n+const title = strings.title\n context"))

        assertEquals(listOf(DiffLine.Kind.Removed, DiffLine.Kind.Added, DiffLine.Kind.Context), diff.lines.map { it.kind })
        assertEquals(listOf("const title = \"Restore\"", "const title = strings.title", "context"), diff.lines.map { it.text })
        assertEquals(listOf(null, null, null), diff.lines.map { it.newNumber })
        assertEquals(1, diff.added)
        assertEquals(1, diff.removed)
    }

    @Test
    fun aDiffWithHunkHeadersKeepsItsLineNumbers() {
        val diff = walkthroughDiff(WalkthroughFile("a.ts", "@@ -10,2 +10,2 @@\n-old\n+new\n same"))

        assertEquals(10, diff.lines[1].oldNumber)
        assertEquals(10, diff.lines[2].newNumber)
    }

    @Test
    fun theStepPositionIsCountedFromOne() {
        assertEquals("Step 2 of 4", stepPosition(1, 4))
    }

    @Test
    fun theGoalAndWalkthroughCallsUseTheConversationsAddresses() = runTest {
        val seen = mutableListOf<Triple<String, String, String>>()
        val engine = MockEngine { request ->
            seen += Triple(request.method.value, request.url.encodedPath, String(request.body.toByteArray()))
            val body = when {
                request.method == HttpMethod.Delete -> ""
                request.url.encodedPath.endsWith("/walkthrough") -> "null"
                request.method == HttpMethod.Get -> "null"
                else -> """{"conversation_id":"c1","text":"Every test passes","token_budget":5000000,"tokens_used":0,"turns":0,"state":"paused"}"""
            }

            respond(body, if (request.method == HttpMethod.Delete) HttpStatusCode.NoContent else HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val api = MyceliumApi("https://mycelium.test", "token", engine)

        val none = api.goal("c1")
        val set = api.setGoal("c1", SetGoalRequest("Every test passes", 5_000_000))
        val paused = api.pauseGoal("c1", true)
        api.clearGoal("c1")
        val walkthrough = api.walkthrough("c1")

        assertNull(none)
        assertNull(walkthrough)
        assertEquals(GoalState.Paused, set.state)
        assertEquals(GoalState.Paused, paused.state)
        assertEquals(
            listOf(
                Triple("GET", "/api/v1/conversations/c1/goal", ""),
                Triple("PUT", "/api/v1/conversations/c1/goal", """{"text":"Every test passes","token_budget":5000000}"""),
                Triple("PATCH", "/api/v1/conversations/c1/goal", """{"paused":true}"""),
                Triple("DELETE", "/api/v1/conversations/c1/goal", ""),
                Triple("GET", "/api/v1/conversations/c1/walkthrough", ""),
            ),
            seen,
        )
    }
}
