package app.flashhop

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import java.io.File
import java.io.FileNotFoundException

/** Serves the last received clipboard image to whichever app pastes it. Read-only, not exported:
 *  the clipboard grants a reader access to just this uri. */
class ClipProvider : ContentProvider() {
    companion object {
        /** Makes [f] the one servable file (saved, since the clipboard outlives this process) and
         *  returns its uri. */
        fun share(ctx: Context, f: File): Uri {
            ctx.getSharedPreferences("flashhop", Context.MODE_PRIVATE).edit().putString("clip", f.path).commit()
            return Uri.parse("content://app.flashhop.clip/${Uri.encode(f.name)}")
        }
    }

    private fun current(uri: Uri): File {
        val path = context!!.getSharedPreferences("flashhop", Context.MODE_PRIVATE).getString("clip", null)
        return path?.let(::File)?.takeIf { it.name == uri.lastPathSegment && it.isFile } ?: throw FileNotFoundException(uri.toString())
    }

    override fun onCreate() = true
    override fun getType(uri: Uri) = "image/png"
    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor =
        ParcelFileDescriptor.open(current(uri), ParcelFileDescriptor.MODE_READ_ONLY)

    override fun query(uri: Uri, projection: Array<String>?, sel: String?, args: Array<String>?, sort: String?): Cursor {
        val f = current(uri)
        val cols = projection ?: arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
        return MatrixCursor(cols, 1).apply {
            addRow(cols.map { if (it == OpenableColumns.SIZE) f.length() else if (it == OpenableColumns.DISPLAY_NAME) f.name else null })
        }
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, sel: String?, args: Array<String>?) = 0
    override fun update(uri: Uri, values: ContentValues?, sel: String?, args: Array<String>?) = 0
}
