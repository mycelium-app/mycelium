package mycelium.state

import mycelium.api.MovePlan

fun goesWithText(plan: MovePlan): String {
    val parts = buildList {
        add("the branch")

        if (plan.changedFiles > 0) {
            add(if (plan.changedFiles == 1) "1 changed file" else "${plan.changedFiles} changed files")
        }

        if (plan.agentSession) {
            add("the agent's session")
        }
    }
    val cloned = if (plan.clonesProject) " The project is cloned there first." else ""

    return "Goes with it: ${joinedWithAnd(parts)}.$cloned"
}

fun staysHereText(plan: MovePlan): String {
    val ports = when (plan.staysPorts.size) {
        0 -> null
        1 -> "port ${plan.staysPorts.first()}"
        else -> "ports ${joinedWithAnd(plan.staysPorts.map { it.toString() })}"
    }
    val processes = when (plan.staysProcesses.size) {
        0 -> null
        1 -> "the ${plan.staysProcesses.first()} process"
        else -> "the ${joinedWithAnd(plan.staysProcesses)} processes"
    }
    val left = listOfNotNull(ports, processes)

    return if (left.isEmpty()) "Nothing is left running here." else "Stays here: ${joinedWithAnd(left)}."
}

private fun joinedWithAnd(parts: List<String>): String = when (parts.size) {
    0, 1 -> parts.joinToString("")
    else -> parts.dropLast(1).joinToString(", ") + " and " + parts.last()
}
