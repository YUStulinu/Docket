package io.github.yustulinu.docket.ui

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

private val Light = lightColorScheme(
    primary = Color(0xFF1F5FAD),
    secondary = Color(0xFF4F6077),
    tertiary = Color(0xFF8A5A00),
    error = Color(0xFFB3261E),
)
private val Dark = darkColorScheme(
    primary = Color(0xFFA6C8FF),
    secondary = Color(0xFFB7C7DF),
    tertiary = Color(0xFFFFB95C),
)

@Composable
fun DocketTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val colors = when {
        Build.VERSION.SDK_INT >= 31 && dark -> dynamicDarkColorScheme(LocalContext.current)
        Build.VERSION.SDK_INT >= 31 -> dynamicLightColorScheme(LocalContext.current)
        dark -> Dark
        else -> Light
    }
    MaterialTheme(colorScheme = colors, content = content)
}
