package mycelium.state

enum class TokenKind { Plain, Keyword, Text, Comment, Number, Added, Removed }

data class Token(val kind: TokenKind, val text: String)

private data class Grammar(
    val keywords: Set<String>,
    val lineComment: String? = null,
    val blockComment: Boolean = false,
    val quotes: String = "\"",
    val keyed: Char? = null,
)

private fun words(text: String) = text.split(' ').toSet()

private val CLike = "if else for while do return break continue switch case default true false null new this in is as try catch finally throw"

private val Grammars: Map<String, Grammar> = buildMap {
    val kotlin = Grammar(words("$CLike fun val var class object interface when package import override private public protected internal data sealed enum suspend companion init typealias by where"), "//", true, "\"'")
    val rust = Grammar(words("$CLike fn let mut struct enum impl trait pub use mod crate self Self match loop async await move ref static const type where unsafe dyn Some None Ok Err"), "//", true)
    val script = Grammar(words("$CLike function const let var class extends import export from async await typeof instanceof interface type undefined static of yield void delete"), "//", true, "\"'`")
    val python = Grammar(words("def class if elif else for while return import from as with try except finally raise pass break continue lambda yield in is not and or None True False async await global nonlocal del assert"), "#", false, "\"'")
    val shell = Grammar(words("if then else elif fi for while do done case esac function in return exit export local echo cd set unset source true false"), "#", false, "\"'")
    val json = Grammar(words("true false null"))
    val yaml = Grammar(words("true false null yes no"), "#", false, "\"'", ':')
    val toml = Grammar(words("true false"), "#", false, "\"'", '=')

    for (name in listOf("kotlin", "kt", "kts", "java")) put(name, kotlin)
    for (name in listOf("rust", "rs")) put(name, rust)
    for (name in listOf("typescript", "ts", "tsx", "javascript", "js", "jsx", "mjs")) put(name, script)
    for (name in listOf("python", "py")) put(name, python)
    for (name in listOf("bash", "sh", "shell", "zsh", "fish", "console")) put(name, shell)
    put("json", json)
    for (name in listOf("yaml", "yml")) put(name, yaml)
    put("toml", toml)
}

private val Key = Regex("[A-Za-z0-9_.\\-\"']+")

fun highlight(language: String, code: String): List<Token> {
    val name = language.trim().lowercase()

    if (name == "diff" || name == "patch") {
        return code.split("\n").mapIndexed { index, line ->
            val kind = when {
                line.startsWith("@@") -> TokenKind.Keyword
                line.startsWith("+") -> TokenKind.Added
                line.startsWith("-") -> TokenKind.Removed
                else -> TokenKind.Plain
            }

            Token(kind, if (index == 0) line else "\n$line")
        }
    }

    val grammar = Grammars[name] ?: return listOf(Token(TokenKind.Plain, code))
    val tokens = mutableListOf<Token>()
    var index = 0

    fun emit(kind: TokenKind, end: Int) {
        val text = code.substring(index, end)
        val last = tokens.lastOrNull()

        if (last != null && last.kind == kind) {
            tokens[tokens.lastIndex] = Token(kind, last.text + text)
        } else {
            tokens += Token(kind, text)
        }

        index = end
    }

    fun lineEnd(from: Int) = code.indexOf('\n', from).let { if (it < 0) code.length else it }

    while (index < code.length) {
        val char = code[index]
        val before = code.substring(code.lastIndexOf('\n', index - 1) + 1, index)

        if (grammar.keyed != null && (before.isBlank() || before.trim() == "-") && Key.matchAt(code, index) != null) {
            val key = Key.matchAt(code, index)!!
            val after = key.range.last + 1
            val separator = code.substring(after, lineEnd(after)).trimStart()

            if (separator.startsWith(grammar.keyed)) {
                emit(TokenKind.Keyword, after)
                continue
            }
        }

        if (grammar.keyed == '=' && char == '[' && before.isBlank()) {
            emit(TokenKind.Keyword, lineEnd(index))
            continue
        }

        when {
            grammar.lineComment != null && code.startsWith(grammar.lineComment, index) -> emit(TokenKind.Comment, lineEnd(index))
            grammar.blockComment && code.startsWith("/*", index) -> emit(TokenKind.Comment, code.indexOf("*/", index + 2).let { if (it < 0) code.length else it + 2 })
            char in grammar.quotes -> {
                var end = index + 1

                while (end < code.length && code[end] != char && (code[end] != '\n' || char == '`')) {
                    end += if (code[end] == '\\') 2 else 1
                }

                end = minOf(end, code.length)

                emit(TokenKind.Text, if (end < code.length && code[end] == char) end + 1 else end)
            }
            char.isDigit() && (index == 0 || !(code[index - 1].isLetterOrDigit() || code[index - 1] == '_')) -> {
                var end = index

                while (end < code.length && (code[end].isLetterOrDigit() || code[end] == '_' || (code[end] == '.' && end + 1 < code.length && code[end + 1].isDigit()))) {
                    end++
                }

                emit(TokenKind.Number, end)
            }
            char.isLetter() || char == '_' -> {
                var end = index

                while (end < code.length && (code[end].isLetterOrDigit() || code[end] == '_')) {
                    end++
                }

                emit(if (code.substring(index, end) in grammar.keywords) TokenKind.Keyword else TokenKind.Plain, end)
            }
            else -> emit(TokenKind.Plain, index + 1)
        }
    }

    return tokens
}
