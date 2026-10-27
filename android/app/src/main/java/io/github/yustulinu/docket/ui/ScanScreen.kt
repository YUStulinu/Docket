package io.github.yustulinu.docket.ui

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageCapture
import androidx.camera.core.ImageCaptureException
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material.icons.filled.PhotoLibrary
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import io.github.yustulinu.docket.R
import io.github.yustulinu.docket.data.Repository
import io.github.yustulinu.docket.scan.NativeScan
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.docket_core.PageInput

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ScanScreen(session: ScanSession, onBack: () -> Unit, onDone: () -> Unit) {
    val context = LocalContext.current
    var hasCamera by remember {
        mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED)
    }
    val askCamera = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { hasCamera = it }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia(10)) { uris ->
        for (u in uris) context.contentResolver.openInputStream(u)?.use { session.photos += it.readBytes() }
    }
    var capture by remember { mutableStateOf<ImageCapture?>(null) }
    var quad by remember { mutableStateOf<FloatArray?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.scan)) },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            Box(Modifier.weight(1f).fillMaxWidth()) {
                if (hasCamera) {
                    CameraPreview(onCapture = { capture = it }, onQuad = { quad = it })
                    QuadOverlay(quad)
                    val found = (quad?.get(8) ?: 0f) > 0f
                    Text(
                        stringResource(if (found) R.string.page_found else R.string.looking_for_page),
                        color = Color.White,
                        modifier = Modifier
                            .align(Alignment.TopCenter)
                            .padding(12.dp)
                            .background(Color(0x99000000), RoundedCornerShape(50))
                            .padding(horizontal = 14.dp, vertical = 6.dp),
                    )
                } else {
                    Column(Modifier.align(Alignment.Center).padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(stringResource(R.string.camera_permission))
                        Spacer(Modifier.size(12.dp))
                        Button(onClick = { askCamera.launch(Manifest.permission.CAMERA) }) { Text(stringResource(R.string.grant)) }
                    }
                }
            }
            LazyRow(Modifier.fillMaxWidth().height(88.dp).padding(8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                itemsIndexed(session.photos) { _, bytes ->
                    val thumb = remember(bytes) {
                        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = 16 })?.asImageBitmap()
                    }
                    thumb?.let { Image(it, null, Modifier.size(72.dp)) }
                }
            }
            Row(Modifier.fillMaxWidth().padding(16.dp), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                OutlinedButton(
                    onClick = { pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
                    modifier = Modifier.testTag("gallery"),
                ) {
                    Icon(Icons.Filled.PhotoLibrary, null)
                    Text(" " + stringResource(R.string.gallery))
                }
                FilledIconButton(
                    onClick = {
                        capture?.takePicture(
                            ContextCompat.getMainExecutor(context),
                            object : ImageCapture.OnImageCapturedCallback() {
                                override fun onCaptureSuccess(image: ImageProxy) {
                                    session.photos += jpegOf(image)
                                    image.close()
                                }

                                override fun onError(exception: ImageCaptureException) {}
                            },
                        )
                    },
                    enabled = hasCamera,
                    modifier = Modifier.size(64.dp),
                ) { Icon(Icons.Filled.CameraAlt, stringResource(R.string.capture)) }
                Button(onClick = onDone, enabled = session.photos.isNotEmpty(), modifier = Modifier.testTag("done")) {
                    Text(stringResource(R.string.done_pages, session.photos.size))
                }
            }
        }
    }
}

/** CameraX gives a JPEG plane for captures; its EXIF orientation is applied by the native decoder. */
private fun jpegOf(image: ImageProxy): ByteArray {
    val buf = image.planes[0].buffer
    val bytes = ByteArray(buf.remaining())
    buf.get(bytes)
    if (image.format == android.graphics.ImageFormat.JPEG) return bytes
    val bmp = image.toBitmap()
    return ByteArrayOutputStream().also { bmp.compress(android.graphics.Bitmap.CompressFormat.JPEG, 92, it) }.toByteArray()
}

@Composable
private fun CameraPreview(onCapture: (ImageCapture) -> Unit, onQuad: (FloatArray?) -> Unit) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val analysisExecutor = remember { Executors.newSingleThreadExecutor() }
    DisposableEffect(Unit) { onDispose { analysisExecutor.shutdown() } }
    AndroidView(
        factory = { ctx ->
            val view = PreviewView(ctx).apply {
                scaleType = PreviewView.ScaleType.FIT_CENTER
                // A TextureView, so that the page outline and the hint are drawn over the preview.
                implementationMode = PreviewView.ImplementationMode.COMPATIBLE
            }
            val providerFuture = ProcessCameraProvider.getInstance(ctx)
            providerFuture.addListener({
                val provider = providerFuture.get()
                val preview = Preview.Builder().build().also { it.surfaceProvider = view.surfaceProvider }
                val capture = ImageCapture.Builder().setCaptureMode(ImageCapture.CAPTURE_MODE_MAXIMIZE_QUALITY).build()
                var last = 0L
                val analysis = ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
                analysis.setAnalyzer(analysisExecutor) { frame ->
                    val now = System.currentTimeMillis()
                    if (now - last > 250) { // a few outlines a second is enough
                        last = now
                        val plane = frame.planes[0]
                        val buf = plane.buffer
                        val bytes = ByteArray(buf.remaining()).also { buf.get(it) }
                        val q = NativeScan.detectQuad(bytes, frame.width, frame.height, plane.rowStride)
                        onQuad(rotate(q, frame.imageInfo.rotationDegrees))
                    }
                    frame.close()
                }
                provider.unbindAll()
                provider.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, preview, capture, analysis)
                onCapture(capture)
            }, ContextCompat.getMainExecutor(context))
            view
        },
        modifier = Modifier.fillMaxSize(),
    )
}

/** Corner fractions of the sensor frame, rotated into the upright preview. */
private fun rotate(q: FloatArray, degrees: Int): FloatArray {
    val out = q.copyOf()
    for (i in 0 until 4) {
        val x = q[2 * i]
        val y = q[2 * i + 1]
        val (nx, ny) = when (degrees) {
            90 -> (1 - y) to x
            180 -> (1 - x) to (1 - y)
            270 -> y to (1 - x)
            else -> x to y
        }
        out[2 * i] = nx
        out[2 * i + 1] = ny
    }
    return out
}

@Composable
private fun QuadOverlay(q: FloatArray?) {
    if (q == null || q[8] <= 0f) return
    val color = MaterialTheme.colorScheme.primary
    Canvas(Modifier.fillMaxSize()) {
        val p = Path()
        for (i in 0 until 4) {
            val o = Offset(q[2 * i] * size.width, q[2 * i + 1] * size.height)
            if (i == 0) p.moveTo(o.x, o.y) else p.lineTo(o.x, o.y)
        }
        p.close()
        drawPath(p, color.copy(alpha = 0.25f))
        drawPath(p, color, style = Stroke(width = 6f))
    }
}

/** Straightens and reads each photo, then extracts and stores the document. */
@Composable
fun ProcessScreen(repo: Repository, session: ScanSession, onDone: (String) -> Unit, onFailed: () -> Unit) {
    var step by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val stepScan = R.string.step_scan
    val context = LocalContext.current
    LaunchedEffect(Unit) {
        try {
            val pages = mutableListOf<PageInput>()
            for ((i, photo) in session.photos.withIndex()) {
                step = context.getString(stepScan, i + 1, session.photos.size)
                pages += repo.scanPage(photo)
            }
            step = context.getString(
                if (repo.modelState.value is io.github.yustulinu.docket.data.ModelState.Ready) R.string.step_extract_model else R.string.step_extract,
            )
            val doc = withContext(Dispatchers.Default) { repo.addDocument(pages) }
            session.photos.clear()
            onDone(doc.id)
        } catch (e: Exception) {
            error = e.message ?: e.toString()
        }
    }
    Column(Modifier.fillMaxSize().padding(32.dp).testTag("processing"), verticalArrangement = Arrangement.Center) {
        Text(stringResource(R.string.processing), style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.size(16.dp))
        if (error == null) {
            LinearProgressIndicator(Modifier.fillMaxWidth())
            Spacer(Modifier.size(12.dp))
            Text(step)
        } else {
            Text(error!!, color = MaterialTheme.colorScheme.error)
            Spacer(Modifier.size(12.dp))
            Button(onClick = onFailed) { Text(stringResource(R.string.back)) }
        }
    }
}
