package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class TestCalendarsRequest(val url: String, val username: String, val password: String)

@Serializable
data class CalendarAccess(val name: String, @SerialName("read_only") val readOnly: Boolean = false)

@Serializable
data class TestedCalendars(val username: String, val calendars: List<CalendarAccess> = emptyList())
