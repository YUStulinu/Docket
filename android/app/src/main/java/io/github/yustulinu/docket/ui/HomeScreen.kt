package io.github.yustulinu.docket.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.yustulinu.docket.R
import io.github.yustulinu.docket.data.Repository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.docket_core.Reminder
import uniffi.docket_core.Summary
import uniffi.docket_core.Urgency

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(repo: Repository, onScan: () -> Unit, onOpen: (String) -> Unit, onAsk: () -> Unit, onSettings: () -> Unit) {
    val version by repo.version.collectAsState()
    val data by produceState<Pair<List<Reminder>, List<Summary>>?>(null, version) {
        value = withContext(Dispatchers.IO) {
            val today = repo.today()
            // The next reminder of each deadline in the coming four months, and every overdue one.
            val reminders = repo.docket.reminders(today, repo.locale)
                .filter { it.urgency == Urgency.OVERDUE || daysUntil(it.deadlineDate) <= 120 }
                .distinctBy { it.documentId to it.deadlineId }
                .sortedBy { it.deadlineDate }
            reminders to repo.docket.documents(today)
        }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.app_name)) },
                actions = {
                    IconButton(onClick = onAsk) { Icon(Icons.Filled.Search, stringResource(R.string.search)) }
                    IconButton(onClick = onSettings) { Icon(Icons.Filled.Settings, stringResource(R.string.settings)) }
                },
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = onScan,
                icon = { Icon(Icons.Filled.CameraAlt, null) },
                text = { Text(stringResource(R.string.scan)) },
                modifier = Modifier.testTag("scan"),
            )
        },
    ) { padding ->
        val (reminders, docs) = data ?: return@Scaffold
        if (docs.isEmpty()) {
            Column(Modifier.fillMaxSize().padding(padding).padding(32.dp), verticalArrangement = Arrangement.Center) {
                Text(stringResource(R.string.empty_title), style = MaterialTheme.typography.headlineSmall)
                Spacer(Modifier.size(12.dp))
                Text(stringResource(R.string.empty_body), style = MaterialTheme.typography.bodyLarge)
            }
            return@Scaffold
        }
        LazyColumn(
            Modifier.fillMaxSize().padding(padding).testTag("home"),
            contentPadding = PaddingValues(16.dp, 8.dp, 16.dp, 96.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item { SectionTitle(stringResource(R.string.upcoming)) }
            if (reminders.isEmpty()) item { Text(stringResource(R.string.nothing_upcoming), Modifier.padding(vertical = 8.dp)) }
            items(reminders, key = { "r" + it.id }) { r -> DeadlineCard(r) { onOpen(r.documentId) } }
            item { SectionTitle(stringResource(R.string.documents)) }
            items(docs, key = { "d" + it.id }) { d -> DocumentRow(d) { onOpen(d.id) } }
        }
    }
}

@Composable
fun SectionTitle(text: String) {
    Text(text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(top = 16.dp, bottom = 4.dp))
}

@Composable
private fun DeadlineCard(r: Reminder, onClick: () -> Unit) {
    val days = daysUntil(r.deadlineDate)
    val color = when {
        r.urgency == Urgency.OVERDUE || days < 0 -> MaterialTheme.colorScheme.errorContainer
        days <= 14 -> MaterialTheme.colorScheme.tertiaryContainer
        else -> MaterialTheme.colorScheme.surfaceContainer
    }
    Card(Modifier.fillMaxWidth().clickable(onClick = onClick), colors = CardDefaults.cardColors(containerColor = color)) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(r.title, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(r.body, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.width(12.dp))
            Column(horizontalAlignment = Alignment.End) {
                Text(prettyDate(r.deadlineDate), style = MaterialTheme.typography.labelLarge)
                Text(if (days < 0) stringResource(R.string.overdue) else "$days d", style = MaterialTheme.typography.labelMedium)
            }
        }
    }
}

@Composable
private fun DocumentRow(d: Summary, onClick: () -> Unit) {
    Card(Modifier.fillMaxWidth().clickable(onClick = onClick)) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(kindIcon(d.kind), null, tint = MaterialTheme.colorScheme.primary)
            Spacer(Modifier.width(16.dp))
            Column(Modifier.weight(1f)) {
                Text(d.title, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(kindLabel(d.kind), style = MaterialTheme.typography.bodySmall)
            }
            d.nextDeadline?.let { Text(prettyDate(it.date), style = MaterialTheme.typography.labelMedium) }
        }
    }
}
