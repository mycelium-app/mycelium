package mycelium

class KeepingPlatform : Platform {
    val preferences = mutableMapOf<String, String>()

    override val signsInThroughTheBrowser = false
    override val originUrl: String? = null

    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("test", "test", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
    override fun loadPreference(key: String): String? = preferences[key]

    override fun savePreference(key: String, value: String) {
        preferences[key] = value
    }
}
