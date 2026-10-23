package mycelium.state

import kotlin.io.encoding.Base64
import kotlin.io.encoding.ExperimentalEncodingApi

const val MaxAttachmentBytes = 20 * 1024 * 1024

const val MaxAttachmentsPerMessage = 10

class PickedFile(val name: String, val mediaType: String, val bytes: ByteArray)

data class DraftAttachment(val id: Int, val file: PickedFile)

private val ExtensionMediaTypes = mapOf(
    "png" to "image/png",
    "jpg" to "image/jpeg",
    "jpeg" to "image/jpeg",
    "gif" to "image/gif",
    "webp" to "image/webp",
    "svg" to "image/svg+xml",
    "pdf" to "application/pdf",
    "json" to "application/json",
    "md" to "text/markdown",
    "csv" to "text/csv",
    "html" to "text/html",
    "xml" to "text/xml",
    "yaml" to "text/yaml",
    "yml" to "text/yaml",
    "toml" to "text/plain",
    "txt" to "text/plain",
    "log" to "text/plain",
    "kt" to "text/plain",
    "rs" to "text/plain",
    "ts" to "text/plain",
    "js" to "text/plain",
    "py" to "text/plain",
    "sh" to "text/plain",
    "sql" to "text/plain",
)

private fun ByteArray.startsWith(vararg prefix: Int): Boolean = size >= prefix.size && prefix.indices.all { this[it].toInt() and 0xff == prefix[it] }

fun mediaTypeOf(name: String, bytes: ByteArray, declared: String? = null): String {
    val sniffed = when {
        bytes.startsWith(0x89, 0x50, 0x4e, 0x47) -> "image/png"
        bytes.startsWith(0xff, 0xd8, 0xff) -> "image/jpeg"
        bytes.startsWith(0x47, 0x49, 0x46, 0x38) -> "image/gif"
        bytes.startsWith(0x52, 0x49, 0x46, 0x46) && bytes.size > 11 && bytes.decodeToString(8, 12) == "WEBP" -> "image/webp"
        else -> null
    }
    val extension = name.substringAfterLast('.', "").lowercase()
    val declaredType = declared?.substringBefore(';')?.trim()?.lowercase()?.takeIf { it.contains('/') && it != "application/octet-stream" }

    return sniffed ?: declaredType ?: ExtensionMediaTypes[extension] ?: "application/octet-stream"
}

fun isImage(mediaType: String): Boolean = mediaType.startsWith("image/")

fun formatSize(bytes: Long): String {
    val megabyteTenths = bytes * 10 / (1024 * 1024)

    return when {
        bytes < 1024 -> "$bytes B"
        bytes < 1024 * 1024 -> "${bytes / 1024} KB"
        else -> "${megabyteTenths / 10}.${megabyteTenths % 10} MB"
    }
}

fun attachmentProblem(file: PickedFile, alreadyAttached: Int): String? = when {
    alreadyAttached >= MaxAttachmentsPerMessage -> "Attach no more than $MaxAttachmentsPerMessage files to one message. Remove one to add ${file.name}."
    file.bytes.isEmpty() -> "${file.name} is empty. Choose a file that has content."
    file.bytes.size > MaxAttachmentBytes -> "${file.name} is larger than 20 MB. Choose a smaller file."
    else -> null
}

private val ProjectLogoMediaTypes = setOf("image/png", "image/jpeg", "image/webp")

const val MaxProjectLogoBytes = 256 * 1024

fun projectLogoProblem(file: PickedFile): String? = when {
    file.mediaType !in ProjectLogoMediaTypes -> "${file.name} is not a PNG, JPEG or WebP image. Choose another file."
    file.bytes.isEmpty() -> "${file.name} is empty. Choose an image that has content."
    file.bytes.size > MaxProjectLogoBytes -> "${file.name} is larger than 256 KB. Choose a smaller image."
    else -> null
}

@OptIn(ExperimentalEncodingApi::class)
fun decodeBase64(data: String): ByteArray? = runCatching { Base64.decode(data) }.getOrNull()

private val RoundConstants = uintArrayOf(
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u, 0x3956c25bu, 0x59f111f1u, 0x923f82a4u, 0xab1c5ed5u,
    0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u, 0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u,
    0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu, 0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
    0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u, 0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u,
    0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u, 0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u,
    0xa2bfe8a1u, 0xa81a664bu, 0xc24b8b70u, 0xc76c51a3u, 0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
    0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u, 0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u, 0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u,
)

private fun UInt.rotateRight(bits: Int): UInt = (this shr bits) or (this shl (32 - bits))

fun sha256Hex(data: ByteArray): String {
    val padded = data.copyOf((data.size + 9 + 63) / 64 * 64)
    padded[data.size] = 0x80.toByte()
    val bitLength = data.size.toLong() * 8

    for (index in 0 until 8) {
        padded[padded.size - 1 - index] = (bitLength ushr (8 * index)).toByte()
    }

    var state = uintArrayOf(0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au, 0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u)
    val schedule = UIntArray(64)

    for (block in 0 until padded.size / 64) {
        for (word in 0 until 16) {
            val at = block * 64 + word * 4
            schedule[word] = (0 until 4).fold(0u) { acc, offset -> (acc shl 8) or (padded[at + offset].toUInt() and 0xffu) }
        }

        for (word in 16 until 64) {
            val low = schedule[word - 15].rotateRight(7) xor schedule[word - 15].rotateRight(18) xor (schedule[word - 15] shr 3)
            val high = schedule[word - 2].rotateRight(17) xor schedule[word - 2].rotateRight(19) xor (schedule[word - 2] shr 10)
            schedule[word] = schedule[word - 16] + low + schedule[word - 7] + high
        }

        val working = state.copyOf()

        for (round in 0 until 64) {
            val sigmaE = working[4].rotateRight(6) xor working[4].rotateRight(11) xor working[4].rotateRight(25)
            val choice = (working[4] and working[5]) xor (working[4].inv() and working[6])
            val first = working[7] + sigmaE + choice + RoundConstants[round] + schedule[round]
            val sigmaA = working[0].rotateRight(2) xor working[0].rotateRight(13) xor working[0].rotateRight(22)
            val majority = (working[0] and working[1]) xor (working[0] and working[2]) xor (working[1] and working[2])
            val second = sigmaA + majority

            for (slot in 7 downTo 1) {
                working[slot] = working[slot - 1]
            }

            working[4] = working[4] + first
            working[0] = first + second
        }

        state = UIntArray(8) { state[it] + working[it] }
    }

    return state.joinToString("") { it.toString(16).padStart(8, '0') }
}
