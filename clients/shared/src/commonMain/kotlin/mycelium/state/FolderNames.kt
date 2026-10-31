package mycelium.state

import kotlin.random.Random

private val Adjectives = listOf("quiet", "calm", "brisk", "bright", "gentle", "mellow", "swift", "steady", "amber", "mossy", "silver", "tidy", "plucky", "sunny", "wild", "hushed", "nimble", "lucky", "misty", "cosy")

private val Nouns = listOf("otter", "heron", "badger", "falcon", "willow", "maple", "comet", "harbour", "meadow", "pebble", "lantern", "fox", "wren", "cedar", "orchid", "lynx", "marten", "thistle", "brook", "finch")

fun randomFolderName(random: Random = Random): String = "${Adjectives.random(random)}-${Nouns.random(random)}"
