package io.github.yustulinu.docket.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.yustulinu.docket.R
import io.github.yustulinu.docket.data.Repository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.docket_core.Answer

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AskScreen(repo: Repository, onBack: () -> Unit, onCitation: (String, Int, String) -> Unit) {
    val scope = rememberCoroutineScope()
    var question by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var answer by remember { mutableStateOf<Answer?>(null) }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.search)) },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding).padding(16.dp).verticalScroll(rememberScrollState())) {
            OutlinedTextField(
                question, { question = it },
                placeholder = { Text(stringResource(R.string.ask_hint)) },
                modifier = Modifier.fillMaxWidth().testTag("question"),
            )
            Spacer(Modifier.size(8.dp))
            Button(
                onClick = {
                    busy = true
                    scope.launch {
                        answer = withContext(Dispatchers.Default) { repo.docket.ask(question, repo.locale, repo.today()) }
                        busy = false
                    }
                },
                enabled = question.isNotBlank() && !busy,
                modifier = Modifier.testTag("ask"),
            ) { Text(stringResource(R.string.ask)) }
            if (busy) {
                Spacer(Modifier.size(16.dp))
                LinearProgressIndicator(Modifier.fillMaxWidth())
                Text(stringResource(R.string.thinking), style = MaterialTheme.typography.bodySmall)
            }
            answer?.let { a ->
                Spacer(Modifier.size(16.dp))
                Card(Modifier.fillMaxWidth().testTag("answer")) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (a.method == "search" && a.citations.isNotEmpty()) {
                            Text(stringResource(R.string.answer_without_model), style = MaterialTheme.typography.bodySmall)
                        }
                        Text(a.text, style = MaterialTheme.typography.titleMedium)
                        if (!a.verified && a.citations.isNotEmpty()) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Icon(Icons.Filled.Warning, null, tint = MaterialTheme.colorScheme.error)
                                Text(" " + stringResource(R.string.unverified), color = MaterialTheme.colorScheme.error)
                            }
                        }
                        if (a.citations.isNotEmpty()) Text(stringResource(R.string.sources), fontWeight = FontWeight.SemiBold)
                        for (c in a.citations) {
                            Column(Modifier.fillMaxWidth().clickable { onCitation(c.documentId, c.page.toInt(), c.text) }.padding(vertical = 4.dp)) {
                                Text("${c.title} · p. ${c.page}", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
                                Text(c.text, style = MaterialTheme.typography.bodySmall, maxLines = 3)
                            }
                        }
                    }
                }
            }
        }
    }
}
