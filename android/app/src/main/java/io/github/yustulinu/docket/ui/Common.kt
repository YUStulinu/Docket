package io.github.yustulinu.docket.ui

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ReceiptLong
import androidx.compose.material.icons.filled.Badge
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.material.icons.filled.Flight
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material.icons.filled.Build
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import io.github.yustulinu.docket.R
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.temporal.ChronoUnit
import uniffi.docket_core.DocKind
import uniffi.docket_core.Source

@Composable
fun kindLabel(kind: DocKind): String = stringResource(
    when (kind) {
        DocKind.VEHICLE_REGISTRATION -> R.string.kind_vehicle_registration
        DocKind.RCA -> R.string.kind_rca
        DocKind.CASCO -> R.string.kind_casco
        DocKind.ITP -> R.string.kind_itp
        DocKind.PASSPORT -> R.string.kind_passport
        DocKind.ID_CARD -> R.string.kind_id_card
        DocKind.DRIVING_LICENCE -> R.string.kind_driving_licence
        DocKind.WARRANTY -> R.string.kind_warranty
        DocKind.RENT_CONTRACT -> R.string.kind_rent_contract
        DocKind.INVOICE -> R.string.kind_invoice
        DocKind.OTHER -> R.string.kind_other
    },
)

fun kindIcon(kind: DocKind): ImageVector = when (kind) {
    DocKind.VEHICLE_REGISTRATION -> Icons.Filled.DirectionsCar
    DocKind.RCA, DocKind.CASCO -> Icons.Filled.Shield
    DocKind.ITP -> Icons.Filled.Build
    DocKind.PASSPORT -> Icons.Filled.Flight
    DocKind.ID_CARD, DocKind.DRIVING_LICENCE -> Icons.Filled.Badge
    DocKind.WARRANTY -> Icons.Filled.Verified
    DocKind.RENT_CONTRACT -> Icons.Filled.Home
    DocKind.INVOICE -> Icons.AutoMirrored.Filled.ReceiptLong
    DocKind.OTHER -> Icons.Filled.Description
}

@Composable
fun sourceLabel(source: Source): String = stringResource(
    when (source) {
        Source.USER -> R.string.set_by_you
        Source.MODEL -> R.string.by_model
        Source.RULES -> R.string.by_rules
    },
)

private val longDate = DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)

fun prettyDate(iso: String): String = runCatching { LocalDate.parse(iso).format(longDate) }.getOrDefault(iso)

/** Days from today to `iso` (negative: in the past). */
fun daysUntil(iso: String): Long = runCatching { ChronoUnit.DAYS.between(LocalDate.now(), LocalDate.parse(iso)) }.getOrDefault(0)
