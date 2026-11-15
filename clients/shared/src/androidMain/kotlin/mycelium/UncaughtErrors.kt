package mycelium

private var uncaughtReport: ((Throwable) -> Unit)? = null

actual fun reportUncaughtErrors(report: (Throwable) -> Unit) {
    if (uncaughtReport == null) {
        val previous = Thread.getDefaultUncaughtExceptionHandler()

        Thread.setDefaultUncaughtExceptionHandler { thread, failure ->
            runCatching { uncaughtReport?.invoke(failure) }
            previous?.uncaughtException(thread, failure)
        }
    }

    uncaughtReport = report
}
