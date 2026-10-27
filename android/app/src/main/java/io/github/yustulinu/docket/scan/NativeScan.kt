package io.github.yustulinu.docket.scan

/** The C++ library (native/): page detection and clean-up, OCR, and the local language model. */
object NativeScan {
    init {
        System.loadLibrary("docket_native")
    }

    /** Straightens, cleans and reads a photo: [OCR JSON (String), corrected page (JPEG bytes)]. */
    external fun scan(photo: ByteArray, detect: Boolean, tessdata: String): Array<Any>

    /** The page in a camera frame's luminance: 4 corners as fractions of the frame, then a score (0: none). */
    external fun detectQuad(luma: ByteArray, width: Int, height: Int, rowStride: Int): FloatArray

    external fun llmOpen(path: String, context: Int, threads: Int): Long

    external fun llmComplete(handle: Long, system: String, user: String, schema: String, maxTokens: Int): String

    external fun llmName(handle: Long): String

    external fun llmClose(handle: Long)
}
