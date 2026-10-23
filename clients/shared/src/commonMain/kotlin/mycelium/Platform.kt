package mycelium

import mycelium.state.PickedFile

data class Connection(val baseUrl: String, val token: String)

data class DeviceInfo(val label: String, val hostname: String, val os: String, val arch: String)

interface Platform {
    val signsInThroughTheBrowser: Boolean

    val originUrl: String?

    val reducedMotion: Boolean get() = false

    fun loadConnection(): Connection?

    fun saveConnection(connection: Connection?)

    fun device(): DeviceInfo

    fun approveCode(): String?

    fun currentPath(): String

    fun navigate(url: String)

    fun openUrl(url: String)

    fun copyToClipboard(text: String)

    fun timeZone(): String? = null

    fun loadPreference(key: String): String? = null

    fun savePreference(key: String, value: String) = Unit

    fun systemAccent(dark: Boolean): Long? = null

    fun pickFiles(onPicked: (List<PickedFile>) -> Unit) = Unit

    fun clipboardFiles(): List<PickedFile> = emptyList()

    fun listenForFiles(listener: ((List<PickedFile>) -> Unit)?) = Unit
}
