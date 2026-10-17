package com.inkframe.app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/** The panel's six inks (design palette), so the app looks like the frame. */
object Ink {
    val Paper = Color(0xFFEDEBE4)
    val Card = Color(0xFFF6F5F0)
    val Black = Color(0xFF1C1F22)
    val Red = Color(0xFFB4161B)
    val Blue = Color(0xFF1F56B3)
    val Yellow = Color(0xFFE6CC2E)
    val Green = Color(0xFF2E6B3F)
    val Muted = Color(0xFF55595E)
}

private val Light = lightColorScheme(
    primary = Ink.Black,
    onPrimary = Ink.Paper,
    primaryContainer = Ink.Yellow,
    onPrimaryContainer = Ink.Black,
    secondary = Ink.Blue,
    onSecondary = Ink.Paper,
    secondaryContainer = Color(0xFFD5DEEE),
    onSecondaryContainer = Ink.Black,
    tertiary = Ink.Green,
    onTertiary = Ink.Paper,
    error = Ink.Red,
    onError = Ink.Paper,
    errorContainer = Color(0xFFF2D6D6),
    onErrorContainer = Ink.Red,
    background = Ink.Paper,
    onBackground = Ink.Black,
    surface = Ink.Paper,
    onSurface = Ink.Black,
    surfaceVariant = Ink.Card,
    onSurfaceVariant = Ink.Muted,
    surfaceContainer = Ink.Card,
    surfaceContainerLow = Ink.Card,
    surfaceContainerHigh = Color(0xFFE4E1D8),
    outline = Ink.Black,
    outlineVariant = Color(0xFFC9C6BC),
)

private val Dark = darkColorScheme(
    primary = Ink.Paper,
    onPrimary = Ink.Black,
    primaryContainer = Ink.Yellow,
    onPrimaryContainer = Ink.Black,
    secondary = Color(0xFF8FB0EA),
    onSecondary = Ink.Black,
    tertiary = Color(0xFF8CC79C),
    error = Color(0xFFF08A8D),
    background = Color(0xFF141618),
    onBackground = Ink.Paper,
    surface = Color(0xFF141618),
    onSurface = Ink.Paper,
    surfaceVariant = Color(0xFF22262A),
    onSurfaceVariant = Color(0xFFB9BBB5),
    surfaceContainer = Color(0xFF1C1F22),
    outline = Color(0xFFB9BBB5),
    outlineVariant = Color(0xFF3A3F44),
)

/** Square-ish corners, like the panel's colour blocks. */
private val Square = Shapes(
    extraSmall = RoundedCornerShape(2.dp),
    small = RoundedCornerShape(2.dp),
    medium = RoundedCornerShape(4.dp),
    large = RoundedCornerShape(4.dp),
    extraLarge = RoundedCornerShape(8.dp),
)

@Composable
fun InkFrameTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, shapes = Square, content = content)
}
