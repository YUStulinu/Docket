package io.github.yustulinu.docket

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.yustulinu.docket.scan.NativeScan
import io.github.yustulinu.docket.scan.pageInput
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.docket_core.DeadlineKind
import uniffi.docket_core.DocKind
import uniffi.docket_core.Docket
import uniffi.docket_core.DocketException
import uniffi.docket_core.generateKey

/**
 * The whole pipeline on the device, on photos of synthetic documents: the C++ scan and OCR, the Rust
 * core's extraction (rules: no model in this test), the encrypted vault, reminders and the calendar.
 */
@RunWith(AndroidJUnit4::class)
class PipelineTest {
    private val testContext = InstrumentationRegistry.getInstrumentation().context
    private val app = InstrumentationRegistry.getInstrumentation().targetContext

    private fun photo(name: String) = testContext.assets.open(name).use { it.readBytes() }

    private fun tessdata(): String {
        val dir = File(app.cacheDir, "test-tessdata").apply { mkdirs() }
        for (name in app.assets.list("tessdata").orEmpty()) {
            val out = File(dir, name)
            if (!out.exists()) app.assets.open("tessdata/$name").use { i -> out.outputStream().use { i.copyTo(it) } }
        }
        return dir.path
    }

    private fun scan(name: String) = NativeScan.scan(photo(name), true, tessdata()).let { pageInput(it[0] as String, it[1] as ByteArray) }

    @Test
    fun aPhotographedWarrantyBecomesADeadline() {
        val page = scan("warranty-1-p1.jpg")
        assertTrue(page.text, page.text.contains("CERTIFICAT DE GARAN"))
        assertTrue(page.image.size > 10_000)

        val dir = File(app.cacheDir, "test-vault-${System.nanoTime()}")
        val key = generateKey()
        val docket = Docket.create(dir.path, key)
        val doc = docket.addDocument(listOf(page), "2026-10-27")
        assertEquals(DocKind.WARRANTY, doc.kind)
        // Sold on 24.06.2026 with a 36-month warranty.
        assertEquals(DeadlineKind.WARRANTY_END, doc.deadlines.single().kind)
        assertEquals("2029-06-24", doc.deadlines.single().date)

        // Nothing readable on disk; the wrong key does not open it; the right one does.
        for (f in dir.walk().filter { it.isFile && it.name.endsWith(".bin") }) {
            assertFalse(String(f.readBytes(), Charsets.ISO_8859_1).contains("GARAN"))
        }
        try {
            Docket.open(dir.path, generateKey())
            error("opened with the wrong key")
        } catch (_: DocketException.BadKey) {
        }
        val reopened = Docket.open(dir.path, key)
        assertEquals(1, reopened.documents("2026-10-27").size)
        assertTrue(reopened.reminders("2026-10-27", "ro").any { it.deadlineDate == "2029-06-24" })
        assertTrue(reopened.calendar("2026-10-27", "2026-10-27T10:00:00Z", "ro").contains("DTSTART;VALUE=DATE:20290624"))
        dir.deleteRecursively()
    }

    @Test
    fun aRegistrationGivesTheItpAndThePlate() {
        val dir = File(app.cacheDir, "test-vault-${System.nanoTime()}")
        val docket = Docket.create(dir.path, generateKey())
        val doc = docket.addDocument(listOf(scan("talon-2-p1.jpg")), "2026-10-27")
        assertEquals(DocKind.VEHICLE_REGISTRATION, doc.kind)
        assertEquals("2028-05-12", doc.deadlines.single { it.kind == DeadlineKind.ITP_EXPIRY }.date)
        assertEquals("TM-87-NUA", doc.fields.single { it.key == "plate" }.value)
        val answer = docket.ask("când trebuie să fac ITP?", "ro", "2026-10-27")
        assertEquals(doc.id, answer.citations.first().documentId)
        dir.deleteRecursively()
    }

    @Test
    fun theLiveOutlineFindsThePage() {
        // The camera's luminance plane: the photo in grey.
        val bmp = android.graphics.BitmapFactory.decodeByteArray(photo("talon-2-p1.jpg"), 0, photo("talon-2-p1.jpg").size)
        val luma = ByteArray(bmp.width * bmp.height)
        val px = IntArray(bmp.width * bmp.height)
        bmp.getPixels(px, 0, bmp.width, 0, 0, bmp.width, bmp.height)
        for (i in px.indices) {
            val c = px[i]
            luma[i] = (((c shr 16 and 0xff) * 77 + (c shr 8 and 0xff) * 150 + (c and 0xff) * 29) shr 8).toByte()
        }
        val q = NativeScan.detectQuad(luma, bmp.width, bmp.height, bmp.width)
        assertTrue("score ${q[8]}", q[8] > 0f)
    }
}
