package mycelium.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.enableEdgeToEdge
import mycelium.App

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        lateinit var platform: AndroidPlatform
        val picker = registerForActivityResult(ActivityResultContracts.GetMultipleContents()) { uris -> platform.picked(uris) }
        platform = AndroidPlatform(applicationContext) { picker.launch("*/*") }

        setContent {
            App(platform)
        }
    }
}
