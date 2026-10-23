package mycelium.state

const val SignInCallback = "mycelium://signed-in"

private const val EncodedSignInCallback = "mycelium%3A%2F%2Fsigned-in"

fun signInCallbackFrom(requested: String?): String? = if (requested == SignInCallback || requested == EncodedSignInCallback) SignInCallback else null
