package io.github.yustulinu.docket.reminders

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import io.github.yustulinu.docket.DocketApp
import java.time.Duration
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZonedDateTime
import java.util.concurrent.TimeUnit
import uniffi.docket_core.Reminder
import uniffi.docket_core.Urgency

/**
 * Turns the core's reminders into notifications: one WorkManager job per reminder, at 09:00 on
 * its day, named by the reminder's stable id (so planning again replaces instead of duplicating),
 * and a daily job that plans again (dates move on, documents change).
 */
object ReminderScheduler {
    private const val TAG = "docket-reminder"
    private const val PREFS = "reminders"
    private val NOTIFY_AT: LocalTime = LocalTime.of(9, 0)

    fun schedule(context: Context, reminders: List<Reminder>) {
        val wm = WorkManager.getInstance(context)
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val now = ZonedDateTime.now()
        val wanted = mutableSetOf<String>()
        for (r in reminders) {
            if (r.urgency == Urgency.OVERDUE) continue // shown in the app, not notified again and again
            var at = LocalDate.parse(r.date).atTime(NOTIFY_AT).atZone(now.zone)
            if (at.isBefore(now)) {
                if (LocalDate.parse(r.date) != now.toLocalDate() || prefs.getBoolean("sent:${r.id}", false)) continue
                at = now // a reminder for today, planned after 09:00
            }
            wanted += r.id
            val work = OneTimeWorkRequestBuilder<ReminderWorker>()
                .setInitialDelay(Duration.between(now, at).toMillis().coerceAtLeast(0), TimeUnit.MILLISECONDS)
                .setInputData(workDataOf("id" to r.id, "title" to r.title, "body" to r.body, "document" to r.documentId))
                .addTag(TAG)
                .build()
            wm.enqueueUniqueWork(r.id, ExistingWorkPolicy.REPLACE, work)
        }
        // Reminders that are no longer planned (a deadline marked done, a document deleted).
        val before = prefs.getStringSet("planned", emptySet()).orEmpty()
        for (id in before - wanted) wm.cancelUniqueWork(id)
        prefs.edit().putStringSet("planned", wanted).apply()

        val daily = PeriodicWorkRequestBuilder<ReplanWorker>(1, TimeUnit.DAYS).build()
        wm.enqueueUniquePeriodicWork("docket-replan", ExistingPeriodicWorkPolicy.KEEP, daily)
    }

    fun markSent(context: Context, id: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean("sent:$id", true).apply()
    }
}

class ReminderWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val id = inputData.getString("id") ?: return Result.failure()
        Notifications.show(applicationContext, id, inputData.getString("title").orEmpty(), inputData.getString("body").orEmpty(),
            inputData.getString("document").orEmpty())
        ReminderScheduler.markSent(applicationContext, id)
        return Result.success()
    }
}

class ReplanWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        (applicationContext as DocketApp).repository.rescheduleReminders()
        return Result.success()
    }
}
