package app.nicevault.desktop

import android.content.Context
import android.database.Cursor
import android.database.MatrixCursor
import android.os.Build
import android.os.CancellationSignal
import android.os.Handler
import android.os.HandlerThread
import android.os.ParcelFileDescriptor
import android.os.ProxyFileDescriptorCallback
import android.os.storage.StorageManager
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import android.provider.DocumentsContract.Root
import android.provider.DocumentsProvider
import android.system.ErrnoException
import android.system.OsConstants
import org.json.JSONArray
import org.json.JSONObject
import java.io.FileNotFoundException

object NvDocs {
  @Volatile private var loaded = false

  fun ready(): Boolean {
    if (!loaded) {
      try {
        System.loadLibrary("nicevault_app")
        loaded = true
      } catch (_: Throwable) {
      }
    }
    return loaded
  }

  @JvmStatic external fun roots(): String
  @JvmStatic external fun stat(doc: String): String
  @JvmStatic external fun children(doc: String): String
  @JvmStatic external fun isChild(parent: String, doc: String): Boolean
  @JvmStatic external fun open(doc: String, write: Boolean, truncate: Boolean): Long
  @JvmStatic external fun release(doc: String, write: Boolean): Boolean
  @JvmStatic external fun read(doc: String, off: Long, buf: ByteArray, len: Int): Int
  @JvmStatic external fun write(doc: String, off: Long, buf: ByteArray, len: Int): Int
  @JvmStatic external fun size(doc: String): Long
  @JvmStatic external fun flush(doc: String): Boolean
  @JvmStatic external fun create(parent: String, name: String, dir: Boolean): String
  @JvmStatic external fun delete(doc: String): String
  @JvmStatic external fun rename(doc: String, name: String): String
  @JvmStatic external fun move(doc: String, target: String): String
}

class NvDocumentsProvider : DocumentsProvider() {
  companion object {
    fun authority(c: Context) = c.packageName + ".documents"

    private val ROOT_COLS = arrayOf(
      Root.COLUMN_ROOT_ID, Root.COLUMN_FLAGS, Root.COLUMN_ICON, Root.COLUMN_TITLE, Root.COLUMN_SUMMARY,
      Root.COLUMN_DOCUMENT_ID, Root.COLUMN_AVAILABLE_BYTES,
    )
    private val DOC_COLS = arrayOf(
      Document.COLUMN_DOCUMENT_ID, Document.COLUMN_DISPLAY_NAME, Document.COLUMN_MIME_TYPE,
      Document.COLUMN_SIZE, Document.COLUMN_LAST_MODIFIED, Document.COLUMN_FLAGS,
    )
    private val proxy = Build.VERSION.SDK_INT >= Build.VERSION_CODES.O
  }

  override fun onCreate(): Boolean = true

  private fun auth() = authority(context!!)

  private fun ok(json: String): Any? {
    val o = JSONObject(json)
    if (o.has("error")) throw FileNotFoundException(o.optString("message", o.optString("error")))
    return o.opt("ok")
  }

  override fun queryRoots(projection: Array<out String>?): Cursor {
    val c = MatrixCursor(projection ?: ROOT_COLS)
    c.setNotificationUri(context!!.contentResolver, DocumentsContract.buildRootsUri(auth()))
    if (!NvDocs.ready()) return c
    val list = try { ok(NvDocs.roots()) as? JSONArray } catch (_: Exception) { null } ?: return c
    for (i in 0 until list.length()) {
      val r = list.getJSONObject(i)
      c.newRow()
        .add(Root.COLUMN_ROOT_ID, r.getString("root"))
        .add(Root.COLUMN_FLAGS, Root.FLAG_SUPPORTS_CREATE or Root.FLAG_LOCAL_ONLY or Root.FLAG_SUPPORTS_IS_CHILD)
        .add(Root.COLUMN_ICON, R.mipmap.ic_launcher)
        .add(Root.COLUMN_TITLE, r.getString("title"))
        .add(Root.COLUMN_SUMMARY, "NiceVault")
        .add(Root.COLUMN_DOCUMENT_ID, r.getString("doc"))
        .add(Root.COLUMN_AVAILABLE_BYTES, r.getLong("free"))
    }
    return c
  }

  private fun addRow(c: MatrixCursor, r: JSONObject) {
    val dir = r.getBoolean("dir")
    val isRoot = r.getString("doc").endsWith("/1")
    var flags = if (dir) {
      Document.FLAG_DIR_SUPPORTS_CREATE
    } else {
      if (proxy) Document.FLAG_SUPPORTS_WRITE else 0
    }
    if (!isRoot) {
      flags = flags or Document.FLAG_SUPPORTS_DELETE or Document.FLAG_SUPPORTS_RENAME
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) flags = flags or Document.FLAG_SUPPORTS_MOVE
    }
    c.newRow()
      .add(Document.COLUMN_DOCUMENT_ID, r.getString("doc"))
      .add(Document.COLUMN_DISPLAY_NAME, r.getString("name"))
      .add(Document.COLUMN_MIME_TYPE, r.getString("mime"))
      .add(Document.COLUMN_SIZE, if (dir) null else r.getLong("size"))
      .add(Document.COLUMN_LAST_MODIFIED, r.getLong("mtime"))
      .add(Document.COLUMN_FLAGS, flags)
  }

  override fun queryDocument(documentId: String, projection: Array<out String>?): Cursor {
    if (!NvDocs.ready()) throw FileNotFoundException(documentId)
    val c = MatrixCursor(projection ?: DOC_COLS)
    addRow(c, ok(NvDocs.stat(documentId)) as JSONObject)
    return c
  }

  override fun queryChildDocuments(parentDocumentId: String, projection: Array<out String>?, sortOrder: String?): Cursor {
    if (!NvDocs.ready()) throw FileNotFoundException(parentDocumentId)
    val c = MatrixCursor(projection ?: DOC_COLS)
    val list = ok(NvDocs.children(parentDocumentId)) as JSONArray
    for (i in 0 until list.length()) addRow(c, list.getJSONObject(i))
    c.setNotificationUri(context!!.contentResolver, DocumentsContract.buildChildDocumentsUri(auth(), parentDocumentId))
    return c
  }

  override fun isChildDocument(parentDocumentId: String, documentId: String): Boolean =
    NvDocs.ready() && NvDocs.isChild(parentDocumentId, documentId)

  private fun parentChanged(doc: String) {
    val root = doc.substringBefore('/') + "/1"
    context?.contentResolver?.notifyChange(DocumentsContract.buildChildDocumentsUri(auth(), root), null)
  }

  override fun openDocument(documentId: String, mode: String, signal: CancellationSignal?): ParcelFileDescriptor {
    if (!NvDocs.ready()) throw FileNotFoundException(documentId)
    val write = mode.contains('w')
    val truncate = write && (mode == "w" || mode.contains('t'))
    if (write && !proxy) throw UnsupportedOperationException("read-only on this Android version")
    val size = NvDocs.open(documentId, write, truncate)
    if (size < 0) throw FileNotFoundException(documentId)
    if (proxy) {
      val sm = context!!.getSystemService(StorageManager::class.java)
      val thread = HandlerThread("nv-doc").apply { start() }
      val pfdMode = when {
        !write -> ParcelFileDescriptor.MODE_READ_ONLY
        mode.startsWith("r") -> ParcelFileDescriptor.MODE_READ_WRITE
        else -> ParcelFileDescriptor.MODE_WRITE_ONLY
      }
      try {
        return sm.openProxyFileDescriptor(pfdMode, Proxy(documentId, write, thread), Handler(thread.looper))
      } catch (e: Exception) {
        NvDocs.release(documentId, write)
        thread.quitSafely()
        throw FileNotFoundException(e.message)
      }
    }
    val pipe = ParcelFileDescriptor.createReliablePipe()
    Thread {
      val buf = ByteArray(1 shl 16)
      var off = 0L
      ParcelFileDescriptor.AutoCloseOutputStream(pipe[1]).use { out ->
        try {
          while (true) {
            if (signal?.isCanceled == true) break
            val n = NvDocs.read(documentId, off, buf, buf.size)
            if (n < 0) {
              pipe[1].closeWithError("volume locked")
              break
            }
            if (n == 0) break
            out.write(buf, 0, n)
            off += n
          }
        } catch (_: Exception) {
        } finally {
          buf.fill(0)
          NvDocs.release(documentId, false)
        }
      }
    }.start()
    return pipe[0]
  }

  private inner class Proxy(val doc: String, val write: Boolean, val thread: HandlerThread) : ProxyFileDescriptorCallback() {
    private var dirty = false

    override fun onGetSize(): Long {
      val n = NvDocs.size(doc)
      if (n < 0) throw ErrnoException("size", OsConstants.EIO)
      return n
    }

    override fun onRead(offset: Long, size: Int, data: ByteArray): Int {
      val n = NvDocs.read(doc, offset, data, size)
      if (n < 0) throw ErrnoException("read", OsConstants.EIO)
      return n
    }

    override fun onWrite(offset: Long, size: Int, data: ByteArray): Int {
      if (!write) throw ErrnoException("write", OsConstants.EBADF)
      val n = NvDocs.write(doc, offset, data, size)
      if (n < 0) throw ErrnoException("write", OsConstants.EIO)
      dirty = true
      return n
    }

    override fun onFsync() {
      if (write && !NvDocs.flush(doc)) throw ErrnoException("fsync", OsConstants.EIO)
    }

    override fun onRelease() {
      NvDocs.release(doc, write)
      if (dirty) parentChanged(doc)
      thread.quitSafely()
    }
  }

  override fun createDocument(parentDocumentId: String, mimeType: String, displayName: String): String {
    if (!NvDocs.ready()) throw FileNotFoundException(parentDocumentId)
    val id = ok(NvDocs.create(parentDocumentId, displayName, mimeType == Document.MIME_TYPE_DIR)) as String
    context?.contentResolver?.notifyChange(DocumentsContract.buildChildDocumentsUri(auth(), parentDocumentId), null)
    return id
  }

  override fun deleteDocument(documentId: String) {
    if (!NvDocs.ready()) throw FileNotFoundException(documentId)
    ok(NvDocs.delete(documentId))
    parentChanged(documentId)
  }

  override fun renameDocument(documentId: String, displayName: String): String? {
    if (!NvDocs.ready()) throw FileNotFoundException(documentId)
    ok(NvDocs.rename(documentId, displayName))
    parentChanged(documentId)
    return null // the id stays the same
  }

  override fun moveDocument(sourceDocumentId: String, sourceParentDocumentId: String, targetParentDocumentId: String): String {
    if (!NvDocs.ready()) throw FileNotFoundException(sourceDocumentId)
    val id = ok(NvDocs.move(sourceDocumentId, targetParentDocumentId)) as String
    val cr = context?.contentResolver
    cr?.notifyChange(DocumentsContract.buildChildDocumentsUri(auth(), sourceParentDocumentId), null)
    cr?.notifyChange(DocumentsContract.buildChildDocumentsUri(auth(), targetParentDocumentId), null)
    return id
  }
}
