package mycelium

data class Connection(val baseUrl: String, val token: String)

data class DeviceInfo(val label: String, val hostname: String, val os: String, val arch: String)

interface Platform {
    val signsInThroughTheBrowser: Boolean

    val originUrl: String?

    fun loadConnection(): Connection?

    fun saveConnection(connection: Connection?)

    fun device(): DeviceInfo

    fun approveCode(): String?

    fun currentPath(): String

    fun navigate(url: String)

    fun openUrl(url: String)

    fun copyToClipboard(text: String)
}
