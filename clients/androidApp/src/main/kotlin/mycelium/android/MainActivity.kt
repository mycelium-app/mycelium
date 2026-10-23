package mycelium.android

import android.Manifest
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.enableEdgeToEdge
import androidx.browser.auth.AuthTabIntent
import androidx.browser.customtabs.CustomTabsClient
import androidx.browser.customtabs.CustomTabsIntent
import mycelium.App
import mycelium.state.SignInCallback

class MainActivity : ComponentActivity() {
    private var platform: AndroidPlatform? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        lateinit var created: AndroidPlatform
        val picker = registerForActivityResult(ActivityResultContracts.GetMultipleContents()) { uris -> created.picked(uris) }
        val signInTab = AuthTabIntent.registerActivityResultLauncher(this) { result -> created.signInClosed(result.resultCode == AuthTabIntent.RESULT_OK) }
        val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}
        created = AndroidPlatform(applicationContext, { picker.launch("*/*") }, { uri ->
            val browser = CustomTabsClient.getPackageName(this, null)

            if (browser != null && CustomTabsClient.isAuthTabSupported(this, browser)) {
                AuthTabIntent.Builder().build().launch(signInTab, uri, SignInCallback.substringBefore("://"))
            } else {
                CustomTabsIntent.Builder().build().launchUrl(this, uri)
            }
        }, { notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS) })
        platform = created
        openConversationFrom(intent)

        setContent {
            App(created)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openConversationFrom(intent)
    }

    private fun openConversationFrom(intent: Intent?) {
        intent?.getStringExtra(ConversationExtra)?.let { platform?.openedFromNotification(it) }
    }

    override fun onPause() {
        super.onPause()
        platform?.pause()
    }

    override fun onResume() {
        super.onResume()
        platform?.resume()
    }
}
