package app.akhripasta

/** The shared Rust engine (core/ + android/rust/). The UI only reads snapshots and calls these. */
object Core {
    init {
        System.loadLibrary("akhri")
    }

    /** Returns null when the engine is running, or an error message. */
    external fun start(name: String, dir: String): String?
    external fun setName(name: String)
    /** Blocks until something changes after [seen]; returns the new generation. */
    external fun waitChange(seen: Long, timeoutMs: Long): Long
    external fun snapshot(): Array<String>
    /** Headline wording from core: strong part and muted rest, joined by U+001F. */
    external fun headline(peers: Int, items: Int): String
    /** [fds] are detached file descriptors; Rust takes ownership and closes them. */
    external fun send(peer: Long, fds: IntArray, names: Array<String>, texts: Array<String>)
    external fun decide(offer: Long, accept: Boolean)
    external fun cancel(job: Long)
    external fun clearFinished()
}

class Peer(val id: Long, val name: String, val ip: String)
class Offer(val id: Long, val summary: String)
/** [state] is `w` waiting, `r` running, `d` done, `f` failed. */
class Job(val id: Long, val label: String, val state: Char, val incoming: Boolean, val fraction: Float, val status: String) {
    val active get() = state == 'w' || state == 'r'
    val failed get() = state == 'f'
}

/** Parsed `Core.snapshot()` (format documented in android/rust/src/lib.rs). */
class Snap(records: Array<String>) {
    val peers = ArrayList<Peer>()
    val offers = ArrayList<Offer>()
    val jobs = ArrayList<Job>()
    val texts = ArrayList<String>()

    init {
        for (r in records) {
            val f = r.split('\u001f')
            when (f[0]) {
                "P" -> peers += Peer(f[1].toLong(), f[2], f[3])
                "O" -> offers += Offer(f[1].toLong(), f[2])
                "J" -> jobs += Job(f[1].toLong(), f[2], f[3][0], f[4] == "1", f[5].toFloat(), f[6])
                "T" -> texts += r.substring(2)
            }
        }
    }
}
