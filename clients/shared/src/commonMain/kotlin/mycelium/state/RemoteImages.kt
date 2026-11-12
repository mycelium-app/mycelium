package mycelium.state

import mycelium.api.RemoteImageResult

class RemoteImages(
    val fetch: suspend (String) -> RemoteImageResult,
    val load: suspend (String) -> RemoteImageResult,
)
