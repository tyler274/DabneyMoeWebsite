package moe.dabney.personal

import android.graphics.Color
import android.os.Bundle
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity

/**
 * Required Android Health Connect permission rationale & privacy usage screen for
 * `moe.dabney.personal` (Android 13 `ACTION_SHOW_PERMISSIONS_RATIONALE` and
 * Android 14+ `VIEW_PERMISSION_USAGE`).
 */
class PermissionsRationaleActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val density = resources.displayMetrics.density
        val pad = (24 * density).toInt()
        val gap = (14 * density).toInt()

        val container =
            LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(pad, pad * 2, pad, pad)
                setBackgroundColor(Color.parseColor("#020617"))
            }

        val title =
            TextView(this).apply {
                text = "Dabney.moe Personal — Health Connect Privacy"
                textSize = 20f
                setTextColor(Color.parseColor("#F8FAFC"))
            }

        val body =
            TextView(this).apply {
                text =
                    """
                    Dabney.moe Personal requests read-only access to Blood Glucose records in Android Health Connect solely to display your personal continuous glucose monitor (such as Abbott Lingo) trends, ADA 5-band Time-in-Range, and 24-hour Ambulatory Glucose Profile (AGP) inside the app.

                    • 100% On-Device Processing: Blood glucose readings are parsed and charted locally inside device RAM using pure Rust/WebAssembly.
                    • Zero Uploads or Sharing: Readings are never uploaded to external servers, sold, or used for advertising, underwriting, or profiling.
                    • Full Control: You can revoke access anytime in Android Health Connect settings or wipe loaded traces in one tap inside the app.
                    """.trimIndent()
                textSize = 14f
                setTextColor(Color.parseColor("#CBD5E1"))
                setPadding(0, gap, 0, gap * 2)
            }

        val closeBtn =
            Button(this).apply {
                text = "Close"
                setOnClickListener { finish() }
            }

        container.addView(title)
        container.addView(body)
        container.addView(closeBtn)

        val scroll =
            ScrollView(this).apply {
                setBackgroundColor(Color.parseColor("#020617"))
                addView(container)
            }

        setContentView(scroll)
    }
}
