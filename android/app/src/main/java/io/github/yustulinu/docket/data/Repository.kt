package io.github.yustulinu.docket.data

import android.content.Context
import android.net.Uri
import io.github.yustulinu.docket.reminders.ReminderScheduler
import io.github.yustulinu.docket.scan.LocalModel
import io.github.yustulinu.docket.scan.NativeScan
import io.github.yustulinu.docket.scan.pageInput
import java.io.File
import java.time.Instant
import java.time.LocalDate
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.docket_core.Docket
import uniffi.docket_core.Document
import uniffi.docket_core.PageInput

sealed interface ModelState {
    data object Missing : ModelState
    data object Loading : ModelState
    data class Ready(val name: String) : ModelState
    data class Failed(val reason: String) : ModelState
}

/** Everything the app does with the vault, the scanner and the model, off the main thread. */
class Repository(private val context: Context) {
    private val prefs = context.getSharedPreferences("docket", Context.MODE_PRIVATE)
    private val modelMutex = Mutex()
    private var model: LocalModel? = null

    private val _modelState = MutableStateFlow<ModelState>(ModelState.Missing)
    val modelState: StateFlow<ModelState> = _modelState

    /** Bumped when documents change, so that screens reload. */
    private val _version = MutableStateFlow(0)
    val version: StateFlow<Int> = _version

    val docket: Docket by lazy {
        val dir = File(context.filesDir, "vault")
        val key = VaultKey.load(context)
        if (File(dir, "vault.json").exists()) Docket.open(dir.path, key) else Docket.create(dir.path, key)
    }

    var locale: String
        get() = prefs.getString("locale", null) ?: if (java.util.Locale.getDefault().language == "ro") "ro" else "en"
        set(v) {
            prefs.edit().putString("locale", v).apply()
            rescheduleReminders()
        }

    fun today(): String = LocalDate.now().toString()

    /** Tesseract's models, copied out of the APK once. */
    private val tessdata: String by lazy {
        val dir = File(context.filesDir, "tessdata").apply { mkdirs() }
        for (name in context.assets.list("tessdata").orEmpty()) {
            val out = File(dir, name)
            if (!out.exists()) {
                context.assets.open("tessdata/$name").use { i -> out.outputStream().use { i.copyTo(it) } }
            }
        }
        dir.path
    }

    private fun modelDir() = File(context.filesDir, "models").apply { mkdirs() }

    /**
     * The .gguf in the app's private models folder. One left in its external files folder (put
     * there with adb push, or by a file manager) is moved in first: private storage is not shared
     * with anything else, and is not subject to the external folder's file permissions.
     */
    fun modelFile(): File? {
        val external = File(context.getExternalFilesDir(null), "models")
        for (f in external.listFiles().orEmpty().filter { it.name.endsWith(".gguf") }) {
            val dest = File(modelDir(), f.name)
            android.util.Log.i("docket", "moving the model ${f.name} into private storage")
            if (!f.renameTo(dest)) {
                val tmp = File(modelDir(), f.name + ".part")
                f.inputStream().use { i -> tmp.outputStream().use { i.copyTo(it, 1 shl 20) } }
                tmp.renameTo(dest)
                f.delete()
            }
        }
        return modelDir().listFiles().orEmpty().filter { it.name.endsWith(".gguf") }.maxByOrNull { it.length() }
    }

    suspend fun loadModel() = withContext(Dispatchers.IO) {
        modelMutex.withLock {
            if (model != null) return@withLock
            val file = modelFile() ?: run {
                _modelState.value = ModelState.Missing
                return@withLock
            }
            _modelState.value = ModelState.Loading
            try {
                val m = LocalModel(file.path)
                model = m
                docket.setModel(m)
                _modelState.value = ModelState.Ready(m.name())
            } catch (e: RuntimeException) {
                _modelState.value = ModelState.Failed(e.message ?: e.toString())
            }
        }
    }

    /** Copies a model the user picked into the app's private storage, then loads it. */
    suspend fun importModel(uri: Uri) = withContext(Dispatchers.IO) {
        val name = uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.endsWith(".gguf") } ?: "model.gguf"
        val tmp = File(modelDir(), "$name.part")
        context.contentResolver.openInputStream(uri)!!.use { i -> tmp.outputStream().use { i.copyTo(it, 1 shl 20) } }
        tmp.renameTo(File(modelDir(), name))
        loadModel()
    }

    /** Straightens, cleans and reads one photo (a few seconds per page). */
    suspend fun scanPage(photo: ByteArray, detect: Boolean = true): PageInput = withContext(Dispatchers.Default) {
        val r = NativeScan.scan(photo, detect, tessdata)
        pageInput(r[0] as String, r[1] as ByteArray)
    }

    suspend fun addDocument(pages: List<PageInput>): Document = withContext(Dispatchers.Default) {
        loadModel() // a model still loading is waited for, not skipped
        val doc = docket.addDocument(pages, today())
        // How, and how fast; never what (the log is readable by anyone with adb).
        android.util.Log.i("docket", "extracted with ${doc.extraction?.method} in ${doc.extraction?.seconds} s")
        changed()
        doc
    }

    suspend fun <T> edit(f: (Docket) -> T): T = withContext(Dispatchers.IO) {
        val r = f(docket)
        changed()
        r
    }

    fun calendar(): String = docket.calendar(today(), Instant.now().toString(), locale)

    private fun changed() {
        _version.value += 1
        rescheduleReminders()
    }

    fun rescheduleReminders() {
        ReminderScheduler.schedule(context, docket.reminders(today(), locale))
    }
}
