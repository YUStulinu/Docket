package io.github.yustulinu.docket.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import io.github.yustulinu.docket.data.Repository
import kotlinx.coroutines.flow.MutableStateFlow

/** The photos of the document being scanned, shared by the scan and processing screens. */
class ScanSession {
    val photos = mutableStateListOf<ByteArray>()
}

@Composable
fun DocketNav(repo: Repository, openDocument: MutableStateFlow<String?>) {
    val nav = rememberNavController()
    val session = remember { ScanSession() }
    val toOpen by openDocument.collectAsState()
    LaunchedEffect(toOpen) {
        toOpen?.let {
            nav.navigate("doc/$it")
            openDocument.value = null
        }
    }
    NavHost(nav, startDestination = "home") {
        composable("home") {
            HomeScreen(
                repo,
                onScan = { session.photos.clear(); nav.navigate("scan") },
                onOpen = { nav.navigate("doc/$it") },
                onAsk = { nav.navigate("ask") },
                onSettings = { nav.navigate("settings") },
            )
        }
        composable("scan") {
            ScanScreen(session, onBack = { nav.popBackStack() }, onDone = { nav.navigate("process") { popUpTo("home") } })
        }
        composable("process") {
            ProcessScreen(repo, session, onDone = { id -> nav.navigate("doc/$id") { popUpTo("home") } }, onFailed = { nav.popBackStack() })
        }
        composable(
            "doc/{id}?page={page}&quote={quote}",
            arguments = listOf(
                navArgument("id") { type = NavType.StringType },
                navArgument("page") { type = NavType.IntType; defaultValue = 0 },
                navArgument("quote") { type = NavType.StringType; nullable = true; defaultValue = null },
            ),
        ) { e ->
            val a = e.arguments!!
            DocumentScreen(repo, a.getString("id")!!, a.getInt("page"), a.getString("quote"), onBack = { nav.popBackStack() })
        }
        composable("ask") {
            AskScreen(repo, onBack = { nav.popBackStack() }, onCitation = { id, page, quote ->
                nav.navigate("doc/$id?page=$page&quote=${android.net.Uri.encode(quote)}")
            })
        }
        composable("settings") { SettingsScreen(repo, onBack = { nav.popBackStack() }) }
    }
}
