package io.github.yustulinu.docket

import android.app.Application
import io.github.yustulinu.docket.data.Repository
import io.github.yustulinu.docket.reminders.Notifications

class DocketApp : Application() {
    val repository: Repository by lazy { Repository(this) }

    override fun onCreate() {
        super.onCreate()
        Notifications.createChannel(this)
    }
}
