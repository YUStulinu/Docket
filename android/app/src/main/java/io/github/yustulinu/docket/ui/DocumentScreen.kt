package io.github.yustulinu.docket.ui

import android.graphics.BitmapFactory
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Card
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.yustulinu.docket.R
import io.github.yustulinu.docket.data.Repository
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.docket_core.Deadline
import uniffi.docket_core.typicalDeadline
import uniffi.docket_core.Document
import uniffi.docket_core.Page
import uniffi.docket_core.Source

/**
 * A document: its deadlines (with the line each was read from), its details and its pages. Opened
 * from an answer, it scrolls to the cited page and outlines the cited line.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DocumentScreen(repo: Repository, id: String, page: Int, quote: String?, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    val version by repo.version.collectAsState()
    val doc by produceState<Document?>(null, version) { value = withContext(Dispatchers.IO) { runCatching { repo.docket.document(id) }.getOrNull() } }
    var renaming by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    var editing by remember { mutableStateOf<Deadline?>(null) }
    val list = rememberLazyListState()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(doc?.title.orEmpty(), maxLines = 1) },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
                actions = {
                    IconButton(onClick = { renaming = true }) { Icon(Icons.Filled.Edit, stringResource(R.string.rename)) }
                    IconButton(onClick = { deleting = true }) { Icon(Icons.Filled.Delete, stringResource(R.string.delete)) }
                },
            )
        },
    ) { padding ->
        val d = doc ?: return@Scaffold
        // Header, deadlines, details, then the pages: jump to the cited page.
        val pageIndex = 4 + d.deadlines.size + d.fields.size + d.pages.indexOfFirst { it.number == page.toUInt() }
        LaunchedEffect(d.id, page) { if (page > 0 && pageIndex >= 4) list.animateScrollToItem(pageIndex) }
        LazyColumn(
            Modifier.fillMaxSize().padding(padding).testTag("document"),
            state = list,
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    AssistChip(onClick = {}, label = { Text(kindLabel(d.kind)) }, leadingIcon = { Icon(kindIcon(d.kind), null) })
                }
                d.extraction?.let { e ->
                    Text(stringResource(R.string.read_with, e.method, e.seconds), style = MaterialTheme.typography.bodySmall)
                    if (e.rejected.isNotEmpty()) Text(stringResource(R.string.rejected, e.rejected.size), style = MaterialTheme.typography.bodySmall)
                }
            }
            item { SectionTitle(stringResource(R.string.deadlines)) }
            if (d.deadlines.isEmpty()) item { Text(stringResource(R.string.no_deadlines)) }
            items(d.deadlines, key = { it.id }) { dl ->
                DeadlineItem(dl, onEdit = { editing = dl }, onToggle = {
                    scope.launch { repo.edit { it.setDeadlineDone(d.id, dl.id, !dl.done) } }
                })
            }
            item {
                TextButton(onClick = {
                    editing = Deadline(
                        id = "", kind = typicalDeadline(d.kind), date = LocalDate.now().plusMonths(1).toString(),
                        label = d.title, page = 1u, quote = "", source = Source.USER, noticeDays = null, done = false,
                    )
                }) { Text(stringResource(R.string.add_deadline)) }
            }
            item { SectionTitle(stringResource(R.string.details)) }
            items(d.fields, key = { "f" + it.key }) { f ->
                Column {
                    Text(f.key.replace('_', ' '), style = MaterialTheme.typography.labelMedium)
                    Text(f.value, style = MaterialTheme.typography.bodyLarge)
                }
            }
            item { SectionTitle(stringResource(R.string.pages)) }
            items(d.pages, key = { "p" + it.number }) { p ->
                PageImage(repo, d.id, p, highlight = if (p.number == page.toUInt()) quote else null)
            }
        }
    }

    if (renaming) {
        var title by remember { mutableStateOf(doc?.title.orEmpty()) }
        AlertDialog(
            onDismissRequest = { renaming = false },
            title = { Text(stringResource(R.string.rename)) },
            text = { OutlinedTextField(title, { title = it }, label = { Text(stringResource(R.string.title)) }, singleLine = true) },
            confirmButton = { TextButton(onClick = { renaming = false; scope.launch { repo.edit { it.setTitle(id, title) } } }) { Text(stringResource(R.string.save)) } },
            dismissButton = { TextButton(onClick = { renaming = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
    if (deleting) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            text = { Text(stringResource(R.string.delete_confirm)) },
            confirmButton = {
                TextButton(onClick = { deleting = false; scope.launch { repo.edit { it.deleteDocument(id) }; onBack() } }) { Text(stringResource(R.string.delete)) }
            },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
    editing?.let { dl ->
        val initial = runCatching { LocalDate.parse(dl.date).atStartOfDay().toInstant(ZoneOffset.UTC).toEpochMilli() }.getOrNull()
        val state = rememberDatePickerState(initialSelectedDateMillis = initial)
        DatePickerDialog(
            onDismissRequest = { editing = null },
            confirmButton = {
                TextButton(onClick = {
                    val millis = state.selectedDateMillis
                    editing = null
                    if (millis != null) {
                        val date = Instant.ofEpochMilli(millis).atZone(ZoneOffset.UTC).toLocalDate().toString()
                        scope.launch { repo.edit { it.setDeadline(id, dl.copy(date = date)) } }
                    }
                }) { Text(stringResource(R.string.save)) }
            },
            dismissButton = { TextButton(onClick = { editing = null }) { Text(stringResource(R.string.cancel)) } },
        ) { DatePicker(state) }
    }
}

@Composable
private fun DeadlineItem(dl: Deadline, onEdit: () -> Unit, onToggle: () -> Unit) {
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp)) {
            Row {
                Column(Modifier.weight(1f)) {
                    Text(dl.label, fontWeight = FontWeight.SemiBold)
                    Text(prettyDate(dl.date) + if (dl.done) "  ·  " + stringResource(R.string.done_label) else "", style = MaterialTheme.typography.titleMedium)
                }
            }
            if (dl.quote.isNotBlank()) {
                Text(stringResource(R.string.from_page, dl.page.toInt(), dl.quote), style = MaterialTheme.typography.bodySmall)
            }
            Text(sourceLabel(dl.source), style = MaterialTheme.typography.labelSmall)
            Row {
                TextButton(onClick = onEdit) { Text(stringResource(R.string.change_date)) }
                TextButton(onClick = onToggle) { Text(stringResource(if (dl.done) R.string.mark_not_done else R.string.mark_done)) }
            }
        }
    }
}

/** The decrypted page image, with the cited line outlined. */
@Composable
private fun PageImage(repo: Repository, id: String, p: Page, highlight: String?) {
    val bitmap by produceState<android.graphics.Bitmap?>(null, id, p.number) {
        value = withContext(Dispatchers.IO) {
            runCatching { repo.docket.pageImage(id, p.number) }.getOrNull()?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
        }
    }
    val bmp = bitmap ?: return
    val target = highlight?.let { q -> p.lines.firstOrNull { line -> overlaps(line.text, q) } }
    Box(Modifier.fillMaxWidth().aspectRatio(bmp.width.toFloat() / bmp.height)) {
        Image(bmp.asImageBitmap(), null, Modifier.fillMaxSize())
        if (target != null) {
            Canvas(Modifier.fillMaxSize()) {
                // Line boxes are in the OCR image's pixels, which is the stored page.
                val sx = size.width / p.width.toFloat()
                val sy = size.height / p.height.toFloat()
                drawRect(
                    Color(0x55FFC107),
                    topLeft = Offset(target.x.toFloat() * sx - 6, target.y.toFloat() * sy - 6),
                    size = Size(target.w.toFloat() * sx + 12, target.h.toFloat() * sy + 12),
                )
            }
        }
    }
}

private fun norm(s: String) = java.text.Normalizer.normalize(s.lowercase(), java.text.Normalizer.Form.NFD).filter { it.isLetterOrDigit() }

private fun overlaps(line: String, quote: String): Boolean {
    val l = norm(line)
    val q = norm(quote)
    return l.length > 3 && (q.contains(l) || l.contains(q.take(24)))
}
