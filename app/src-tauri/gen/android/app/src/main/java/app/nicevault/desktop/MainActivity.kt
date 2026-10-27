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
  override val handleBackNavigation: Boolean = false

  private var web: WebView? = null
  private var bars = floatArrayOf(0f, 0f, 0f, 0f)
  private val main = Handler(Looper.getMainLooper())

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
    val allow = BuildConfig.DEBUG && intent?.getBooleanExtra("nv_allow_screenshots", false) == true
    if (!allow) {
      window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
    }
    ViewCompat.setOnApplyWindowInsetsListener(findViewById(android.R.id.content)) { v, insets ->
      val sys = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
      v.setPadding(0, 0, 0, ime.bottom)
      val d = resources.displayMetrics.density
      bars = floatArrayOf(sys.top / d, sys.right / d, if (ime.bottom > 0) 0f else sys.bottom / d, sys.left / d)
      pushInsets()
      WindowInsetsCompat.CONSUMED
    }
  }
}
