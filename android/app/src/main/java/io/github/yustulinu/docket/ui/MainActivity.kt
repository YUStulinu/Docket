package io.github.yustulinu.docket.ui

import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.lifecycle.lifecycleScope
import io.github.yustulinu.docket.DocketApp
import io.github.yustulinu.docket.reminders.Notifications
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    /** A document to open, from a notification. */
    private val openDocument = MutableStateFlow<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val repo = (application as DocketApp).repository
        openDocument.value = intent.getStringExtra(Notifications.EXTRA_DOCUMENT)
        if (Build.VERSION.SDK_INT >= 33) {
            registerForActivityResult(ActivityResultContracts.RequestPermission()) {}.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        lifecycleScope.launch {
            repo.rescheduleReminders()
            repo.loadModel()
        }
        setContent {
            DocketTheme {
                DocketNav(repo, openDocument)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openDocument.value = intent.getStringExtra(Notifications.EXTRA_DOCUMENT)
    }
}
