package io.github.yustulinu.docket.scan

import uniffi.docket_core.DocketException
import uniffi.docket_core.LanguageModel

/**
 * The language model running on the phone (llama.cpp through JNI), handed to the Rust core, which
 * builds the prompts and checks the answers. One request at a time: the model's context is not
 * shared.
 */
class LocalModel(path: String, context: Int = 4096) : LanguageModel, AutoCloseable {
    private val handle: Long = NativeScan.llmOpen(path, context, 0)
    private val modelName: String = NativeScan.llmName(handle)
    private val lock = Any()

    override fun complete(system: String, user: String, jsonSchema: String): String = synchronized(lock) {
        try {
            val t = System.nanoTime()
            NativeScan.llmComplete(handle, system, user, jsonSchema, 1024).also {
                android.util.Log.i("docket", "model answered ${it.length} chars in ${(System.nanoTime() - t) / 1_000_000} ms")
            }
        } catch (e: RuntimeException) {
            throw DocketException.Model(e.message ?: "the model failed")
        }
    }

    override fun name(): String = modelName

    override fun close() = synchronized(lock) { NativeScan.llmClose(handle) }
}
