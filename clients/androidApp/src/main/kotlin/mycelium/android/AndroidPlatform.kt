package mycelium.android

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.state.MaxAttachmentBytes
import mycelium.state.PickedFile
import mycelium.state.mediaTypeOf

class AndroidPlatform(private val context: Context, private val launchPicker: () -> Unit = {}) : Platform {
    private var waitingForPick: ((List<PickedFile>) -> Unit)? = null

    private val preferences = context.getSharedPreferences("connection", Context.MODE_PRIVATE)
    private val settings = context.getSharedPreferences("settings", Context.MODE_PRIVATE)

    override val signsInThroughTheBrowser = false

    override val originUrl: String? = null

    override fun loadConnection(): Connection? {
        val baseUrl = preferences.getString("baseUrl", null)
        val token = preferences.getString("token", null)

        if (baseUrl == null || token == null) {
            return null
        }

        return Connection(baseUrl, token)
    }

    override fun saveConnection(connection: Connection?) {
        val editor = preferences.edit()

        if (connection == null) {
            editor.clear()
        } else {
            editor.putString("baseUrl", connection.baseUrl).putString("token", connection.token)
        }

        editor.apply()
    }

    override fun device(): DeviceInfo = DeviceInfo("Android app on ${Build.MODEL}", Build.MODEL, "android", Build.SUPPORTED_ABIS.firstOrNull() ?: "unknown")

    override fun approveCode(): String? = null

    override fun currentPath(): String = "/"

    override fun navigate(url: String) = openUrl(url)

    override fun openUrl(url: String) {
        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

        context.startActivity(intent)
    }

    override fun timeZone(): String? = java.time.ZoneId.systemDefault().id

    override fun localDateTime(epochMillis: Long): String? = java.time.LocalDateTime.ofInstant(java.time.Instant.ofEpochMilli(epochMillis), java.time.ZoneId.systemDefault()).format(java.time.format.DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm"))

    override fun loadPreference(key: String): String? = settings.getString(key, null)

    override fun savePreference(key: String, value: String) {
        settings.edit().putString(key, value).apply()
    }

    override fun systemAccent(dark: Boolean): Long? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
            return null
        }

        val colour = if (dark) android.R.color.system_accent1_200 else android.R.color.system_accent1_600

        return context.getColor(colour).toLong()
    }

    override fun pickFiles(onPicked: (List<PickedFile>) -> Unit) {
        waitingForPick = onPicked
        launchPicker()
    }

    fun picked(uris: List<Uri>) {
        val files = uris.mapNotNull { uri ->
            runCatching {
                val name = context.contentResolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else null } ?: "attachment"
                val bytes = context.contentResolver.openInputStream(uri)?.use { it.readNBytes(MaxAttachmentBytes + 1) } ?: return@runCatching null

                PickedFile(name, mediaTypeOf(name, bytes, context.contentResolver.getType(uri)), bytes)
            }.getOrNull()
        }

        waitingForPick?.invoke(files)
        waitingForPick = null
    }

    override fun copyToClipboard(text: String) {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

        clipboard.setPrimaryClip(ClipData.newPlainText("mycelium", text))
    }
}
