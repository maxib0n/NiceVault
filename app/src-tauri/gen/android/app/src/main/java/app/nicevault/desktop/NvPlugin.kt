package app.nicevault.desktop

import android.app.Activity
import android.content.ClipData
import android.content.ContentValues
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import androidx.activity.result.ActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

@InvokeArg
class UrisArgs {
  lateinit var uris: Array<String>
}

@InvokeArg
class ShareArgs {
  lateinit var paths: Array<String>
  var title: String? = null
}

@InvokeArg
class GalleryOpenArgs {
  lateinit var name: String
  lateinit var mime: String
  var video: Boolean = false
}

@InvokeArg
class GalleryCloseArgs {
  lateinit var uri: String
  var ok: Boolean = false
}

@InvokeArg
class PickArgs {
  var multiple: Boolean = true
}

@TauriPlugin
class NvPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun displayNames(invoke: Invoke) {
    val args = invoke.parseArgs(UrisArgs::class.java)
    val names = JSArray()
    for (u in args.uris) {
      var name = ""
      try {
        activity.contentResolver.query(Uri.parse(u), null, null, null, null)?.use { c ->
          if (c.moveToFirst()) {
            val col = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (col >= 0) name = c.getString(col) ?: ""
            name = pickerName(name, c)
          }
        }
      } catch (_: Exception) {
      }
      names.put(name)
    }
    invoke.resolve(JSObject().put("names", names))
  }

  private fun pickerName(name: String, c: android.database.Cursor): String {
    val stem = name.substringBeforeLast('.')
    if (stem.isEmpty() || !stem.all { it.isDigit() }) return name
    val ext = if (name.contains('.')) "." + name.substringAfterLast('.') else ""
    val tc = c.getColumnIndex("datetaken")
    val taken = if (tc >= 0 && !c.isNull(tc)) c.getLong(tc) else 0L
    if (taken <= 0L) return name
    val mc = c.getColumnIndex("mime_type")
    val mime = if (mc >= 0) c.getString(mc) ?: "" else ""
    val prefix = if (mime.startsWith("video/")) "VID_" else "IMG_"
    return prefix + SimpleDateFormat("yyyyMMdd_HHmmss", Locale.US).format(Date(taken)) + ext
  }

  @Command
  fun share(invoke: Invoke) {
    val args = invoke.parseArgs(ShareArgs::class.java)
    try {
      val auth = activity.packageName + ".fileprovider"
      val uris = ArrayList(args.paths.map { FileProvider.getUriForFile(activity, auth, File(it)) })
      val mimes = args.paths.map { p ->
        MimeTypeMap.getSingleton().getMimeTypeFromExtension(p.substringAfterLast('.', "").lowercase()) ?: "application/octet-stream"
      }
      val type = if (mimes.distinct().size == 1) mimes[0] else "*/*"
      val send = if (uris.size == 1) {
        Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, uris[0])
      } else {
        Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
      }
      send.type = type
      val clip = ClipData.newRawUri("", uris[0])
      for (u in uris.drop(1)) clip.addItem(ClipData.Item(u))
      send.clipData = clip
      send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      activity.startActivity(Intent.createChooser(send, args.title))
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject(e.message ?: "share failed")
    }
  }

  @Command
  fun pickPhotos(invoke: Invoke) {
    val args = invoke.parseArgs(PickArgs::class.java)
    try {
      val req = PickVisualMediaRequest.Builder().setMediaType(ActivityResultContracts.PickVisualMedia.ImageAndVideo).build()
      val intent = if (args.multiple) {
        ActivityResultContracts.PickMultipleVisualMedia().createIntent(activity, req)
      } else {
        ActivityResultContracts.PickVisualMedia().createIntent(activity, req)
      }
      startActivityForResult(invoke, intent, "photosPicked")
    } catch (e: Exception) {
      invoke.reject(e.message ?: "photo picker failed")
    }
  }

  @ActivityCallback
  fun photosPicked(invoke: Invoke, result: ActivityResult) {
    val files = JSArray()
    if (result.resultCode == Activity.RESULT_OK) {
      ActivityResultContracts.PickMultipleVisualMedia().parseResult(result.resultCode, result.data).forEach { files.put(it.toString()) }
    }
    invoke.resolve(JSObject().put("files", files))
  }

  @Command
  fun galleryOpen(invoke: Invoke) {
    val args = invoke.parseArgs(GalleryOpenArgs::class.java)
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
      invoke.reject("The gallery needs Android 10 or later.", "unsupported")
      return
    }
    try {
      val collection = if (args.video) {
        MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
      } else {
        MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
      }
      val dir = (if (args.video) Environment.DIRECTORY_MOVIES else Environment.DIRECTORY_PICTURES) + "/NiceVault"
      val values = ContentValues().apply {
        put(MediaStore.MediaColumns.DISPLAY_NAME, args.name)
        put(MediaStore.MediaColumns.MIME_TYPE, args.mime)
        put(MediaStore.MediaColumns.RELATIVE_PATH, dir)
        put(MediaStore.MediaColumns.IS_PENDING, 1)
      }
      val uri = activity.contentResolver.insert(collection, values) ?: throw Exception("MediaStore refused the entry")
      val pfd = activity.contentResolver.openFileDescriptor(uri, "w")
      if (pfd == null) {
        activity.contentResolver.delete(uri, null, null)
        throw Exception("no descriptor for the gallery entry")
      }
      invoke.resolve(JSObject().put("uri", uri.toString()).put("fd", pfd.detachFd()))
    } catch (e: Exception) {
      invoke.reject(e.message ?: "gallery failed", "io")
    }
  }

  @Command
  fun galleryClose(invoke: Invoke) {
    val args = invoke.parseArgs(GalleryCloseArgs::class.java)
    try {
      val uri = Uri.parse(args.uri)
      if (args.ok && Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        activity.contentResolver.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
      } else {
        activity.contentResolver.delete(uri, null, null)
      }
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject(e.message ?: "gallery failed", "io")
    }
  }

  @Command
  fun docsChanged(invoke: Invoke) {
    activity.contentResolver.notifyChange(DocumentsContract.buildRootsUri(NvDocumentsProvider.authority(activity)), null)
    invoke.resolve()
  }
}
