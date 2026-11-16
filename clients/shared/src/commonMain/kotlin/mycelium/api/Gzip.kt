package mycelium.api

import io.ktor.client.HttpClientConfig

internal expect fun HttpClientConfig<*>.acceptGzip()
