package mycelium.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.enableEdgeToEdge
import mycelium.App

class MainActivity : ComponentActivity() {
    private var platform: AndroidPlatform? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        lateinit var created: AndroidPlatform
        val picker = registerForActivityResult(ActivityResultContracts.GetMultipleContents()) { uris -> created.picked(uris) }
        created = AndroidPlatform(applicationContext) { picker.launch("*/*") }
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
