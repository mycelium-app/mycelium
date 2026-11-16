package mycelium.api

import io.ktor.client.HttpClientConfig
import io.ktor.client.plugins.compression.ContentEncoding

internal actual fun HttpClientConfig<*>.acceptGzip() {
    install(ContentEncoding) { gzip() }
}
