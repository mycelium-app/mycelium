package mycelium.android

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
        created = AndroidPlatform(applicationContext, { picker.launch("*/*") }) { uri ->
            val browser = CustomTabsClient.getPackageName(this, null)

            if (browser != null && CustomTabsClient.isAuthTabSupported(this, browser)) {
                AuthTabIntent.Builder().build().launch(signInTab, uri, SignInCallback.substringBefore("://"))
            } else {
                CustomTabsIntent.Builder().build().launchUrl(this, uri)
            }
        }
        platform = created

        setContent {
            App(created)
        }
    }

    override fun onResume() {
        super.onResume()
        platform?.resume()
    }
}
