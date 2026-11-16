package mycelium.api

import io.ktor.client.HttpClientConfig

internal actual fun HttpClientConfig<*>.acceptGzip() = Unit
