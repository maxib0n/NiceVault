package app.nicevault.desktop

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.WindowManager
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  // Back goes to the page (window.__nvBack, app/ui/src/lib/mobile.ts): it closes the preview, a
  // dialog, a menu, an entry, Impostazioni... When the page has nothing to go back from, the app
  // goes to the background (it is not finished: open volumes follow the auto-lock rules).
  override val handleBackNavigation: Boolean = false

  private var web: WebView? = null
  /** the system bars and the cutout, in CSS px: top, right, bottom, left */
  private var bars = floatArrayOf(0f, 0f, 0f, 0f)
  private val main = Handler(Looper.getMainLooper())

  /** The page draws under the bars (its paper runs to the glass) and pads its content by these
   *  (styles/mobile.css --nv-inset-*: the WebView does not report env(safe-area-inset-*) on every
   *  version). Sent again a few times after a load: a reload starts without them. */
  private fun pushInsets() {
    val w = web ?: return
    val (t, r, b, l) = bars.toList()
    val js = "(function(s){s.setProperty('--nv-inset-top','${t}px');s.setProperty('--nv-inset-right','${r}px');" +
      "s.setProperty('--nv-inset-bottom','${b}px');s.setProperty('--nv-inset-left','${l}px')})(document.documentElement.style)"
    w.evaluateJavascript(js, null)
  }

  override fun onWebViewCreate(webView: WebView) {
    web = webView
    for (ms in longArrayOf(0, 400, 1200, 3000)) main.postDelayed({ pushInsets() }, ms)
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        webView.evaluateJavascript("(window.__nvBack ? window.__nvBack() : false)") { r ->
          if (r != "true") moveTaskToBack(true)
        }
      }
    })
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // No screenshots, screen recording or recents thumbnail of an open volume / password
    //. Debug builds started with the extra nv_allow_screenshots
    // (adb shell am start ... --ez nv_allow_screenshots true) skip it, for the emulator smoke test.
    val allow = BuildConfig.DEBUG && intent?.getBooleanExtra("nv_allow_screenshots", false) == true
    if (!allow) {
      window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
    }
    // Edge to edge: the page runs under the status bar, the cutout and the navigation bar and
    // pads its own content (pushInsets); only the keyboard pushes the page up (the content view
    // is padded by it, so a field near the foot stays above the keys).
    ViewCompat.setOnApplyWindowInsetsListener(findViewById(android.R.id.content)) { v, insets ->
      val sys = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
      v.setPadding(0, 0, 0, ime.bottom)
      val d = resources.displayMetrics.density
      // with the keyboard up the navigation bar is under it: no room kept for it at the foot
      bars = floatArrayOf(sys.top / d, sys.right / d, if (ime.bottom > 0) 0f else sys.bottom / d, sys.left / d)
      pushInsets()
      WindowInsetsCompat.CONSUMED
    }
  }
}
