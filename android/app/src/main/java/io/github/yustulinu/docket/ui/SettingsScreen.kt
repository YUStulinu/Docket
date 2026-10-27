package io.github.yustulinu.docket.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.yustulinu.docket.R
import io.github.yustulinu.docket.data.ModelState
import io.github.yustulinu.docket.data.Repository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(repo: Repository, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val model by repo.modelState.collectAsState()
    var locale by remember { mutableStateOf(repo.locale) }
    var exported by remember { mutableStateOf(false) }
    val importModel = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch { repo.importModel(uri) }
    }
    val exportIcs = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/calendar")) { uri ->
        if (uri != null) scope.launch {
            withContext(Dispatchers.IO) {
                val ics = repo.calendar()
                context.contentResolver.openOutputStream(uri)!!.use { it.write(ics.toByteArray()) }
            }
            exported = true
        }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.settings)) },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding).padding(16.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(R.string.model), style = MaterialTheme.typography.titleMedium)
            Text(
                when (val m = model) {
                    is ModelState.Ready -> stringResource(R.string.model_ready, m.name)
                    ModelState.Loading -> stringResource(R.string.model_loading)
                    is ModelState.Failed -> stringResource(R.string.model_failed, m.reason)
                    ModelState.Missing -> stringResource(R.string.model_missing)
                },
            )
            if (model !is ModelState.Ready) {
                OutlinedButton(onClick = { importModel.launch(arrayOf("*/*")) }) { Text(stringResource(R.string.model_import)) }
            }

            Text(stringResource(R.string.reminder_language), style = MaterialTheme.typography.titleMedium)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilterChip(locale == "en", onClick = { locale = "en"; repo.locale = "en" }, label = { Text(stringResource(R.string.english)) })
                FilterChip(locale == "ro", onClick = { locale = "ro"; repo.locale = "ro" }, label = { Text(stringResource(R.string.romanian)) })
            }

            OutlinedButton(onClick = { exportIcs.launch("docket.ics") }) { Text(stringResource(R.string.export_calendar)) }
            if (exported) Text(stringResource(R.string.exported))

            Text(stringResource(R.string.privacy), style = MaterialTheme.typography.titleMedium)
            Text(stringResource(R.string.privacy_body))
        }
    }
}
