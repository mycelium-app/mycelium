package mycelium.android

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.state.MaxAttachmentBytes
import mycelium.state.PickedFile
import mycelium.state.SignInCallback
import mycelium.state.mediaTypeOf

private const val ConversationsChannel = "conversations"
private const val QuietConversationsChannel = "conversations-quiet"

const val ConversationExtra = "conversation_id"

class AndroidPlatform(private val context: Context, private val launchPicker: () -> Unit = {}, private val launchSignIn: (Uri) -> Unit = {}, private val launchNotificationPermission: () -> Unit = {}) : Platform {
    private var waitingForPick: ((List<PickedFile>) -> Unit)? = null

    private var waitingForSignIn: (() -> Unit)? = null

    private val preferences = context.getSharedPreferences("connection", Context.MODE_PRIVATE)
    private val settings = context.getSharedPreferences("settings", Context.MODE_PRIVATE)

    override val signsInThroughTheBrowser = false

    override val originUrl: String? = null
    override val eventPageSize = 100

    override val signInCallback = SignInCallback

    private val resumed = MutableSharedFlow<Unit>(extraBufferCapacity = 1)

    override val resumes: Flow<Unit> = resumed

    private var foreground = false

    private var notificationOpener: ((String) -> Unit)? = null

    private var pendingOpen: String? = null

    fun resume() {
        foreground = true
        resumed.tryEmit(Unit)
    }

    fun pause() {
        foreground = false
    }

    fun openedFromNotification(conversationId: String) {
        val opener = notificationOpener

        if (opener == null) {
            pendingOpen = conversationId
        } else {
            opener(conversationId)
        }
    }

    override fun windowFocused(): Boolean = foreground

    override fun listenForNotificationOpens(listener: ((String) -> Unit)?) {
        notificationOpener = listener

        val waiting = pendingOpen

        if (listener != null && waiting != null) {
            pendingOpen = null
            listener(waiting)
        }
    }

    override fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            launchNotificationPermission()
        }
    }

    override fun notify(title: String, body: String, conversationId: String, sound: Boolean) {
        val allowed = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

        if (!allowed) {
            return
        }

        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.createNotificationChannel(NotificationChannel(ConversationsChannel, "Conversations", NotificationManager.IMPORTANCE_DEFAULT))
        manager.createNotificationChannel(NotificationChannel(QuietConversationsChannel, "Conversations without sound", NotificationManager.IMPORTANCE_LOW))

        val open = Intent(context, MainActivity::class.java)
            .putExtra(ConversationExtra, conversationId)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        val tap = PendingIntent.getActivity(context, conversationId.hashCode(), open, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

        val notification = Notification.Builder(context, if (sound) ConversationsChannel else QuietConversationsChannel)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle(title)
            .setContentText(body)
            .setContentIntent(tap)
            .setAutoCancel(true)

        manager.notify(conversationId.hashCode(), notification.build())
    }

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

    override fun openSignIn(url: String, onCancelled: () -> Unit) {
        waitingForSignIn = onCancelled
        launchSignIn(Uri.parse(url))
    }

    fun signInClosed(approved: Boolean) {
        val cancelled = waitingForSignIn
        waitingForSignIn = null

        if (!approved) {
            cancelled?.invoke()
        }
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

    @Composable
    override fun WebContent(url: String, token: String?, modifier: Modifier) = AndroidWebContent(url, token, modifier)

    override fun copyToClipboard(text: String) {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

        clipboard.setPrimaryClip(ClipData.newPlainText("mycelium", text))
    }
}
