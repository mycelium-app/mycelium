package mycelium.android

import android.annotation.SuppressLint
import android.content.Context
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.delay
import mycelium.PageReading
import mycelium.ui.LocalRetainedViewKey
import mycelium.ui.RetainedViews
import org.json.JSONTokener
import java.net.URI

private const val SessionCookie = "mycelium_client"

private const val ReadIntervalMillis = 3000L

private fun setSessionCookie(url: String, token: String) {
    val uri = URI(url)
    val secure = if (uri.scheme == "https") "; Secure" else ""
    val manager = CookieManager.getInstance()

    manager.setCookie("${uri.scheme}://${uri.authority}", "$SessionCookie=$token; Path=/$secure; HttpOnly")
    manager.flush()
}

@SuppressLint("SetJavaScriptEnabled")
private fun createWebView(context: Context): WebView = WebView(context).apply {
    settings.javaScriptEnabled = true
    settings.domStorageEnabled = true
    settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
    webViewClient = WebViewClient()
}

private const val RetainedWebViewLimit = 4

private val retainedWebViews = RetainedViews<WebView>(RetainedWebViewLimit) { it.destroy() }

@Composable
fun AndroidWebContent(url: String, token: String?, modifier: Modifier, reading: PageReading? = null) {
    val retainKey = LocalRetainedViewKey.current
    val view = remember { arrayOfNulls<WebView>(1) }

    DisposableEffect(Unit) {
        onDispose {
            if (retainKey == null) {
                view[0]?.destroy()
            }
        }
    }

    LaunchedEffect(reading) {
        while (reading != null) {
            delay(ReadIntervalMillis)

            val webView = view[0] ?: continue
            val address = webView.url ?: continue

            if (reading.allowedUrl(address)) {
                webView.evaluateJavascript(reading.script) { answer -> (JSONTokener(answer).nextValue() as? String)?.let(reading.onRead) }
            }
        }
    }

    AndroidView(
        factory = { context ->
            val created = if (retainKey != null) retainedWebViews.obtain(retainKey) { createWebView(context) } else createWebView(context)

            (created.parent as? ViewGroup)?.removeView(created)
            created.also { view[0] = it }
        },
        modifier = modifier,
        update = { webView ->
            if (webView.tag != url) {
                webView.tag = url

                if (token != null) {
                    setSessionCookie(url, token)
                }

                webView.loadUrl(url)
            }
        },
    )
}
