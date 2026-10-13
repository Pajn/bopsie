package expo.modules.cratieicons

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.drawable.Drawable
import android.util.LruCache
import android.widget.ImageView
import expo.modules.kotlin.AppContext
import expo.modules.kotlin.views.ExpoView

/** Local vector inflation and assignment happen in the same native prop update. */
internal class CratieIconImageView(context: Context) : ImageView(context) {
  init {
    scaleType = ScaleType.FIT_CENTER
    importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
  }

  fun setIcon(name: String) {
    val resourceName = "cratie_icon_${name.replace('-', '_')}"
    val state = states.get(resourceName) ?: run {
      val id = resources.getIdentifier(resourceName, "drawable", context.packageName)
      if (id == 0) { setImageDrawable(null); return }
      context.getDrawable(id)?.constantState?.also { states.put(resourceName, it) }
    }
    // Each view gets its own drawable, so one icon's tint cannot alter another.
    setImageDrawable(state?.newDrawable(resources, context.theme)?.mutate())
  }

  fun setIconTint(color: Int) { imageTintList = ColorStateList.valueOf(color) }

  companion object {
    private val states = LruCache<String, Drawable.ConstantState>(64)
  }
}

class CratieIconView(context: Context, appContext: AppContext) : ExpoView(context, appContext) {
  internal val image = CratieIconImageView(context)

  init { addView(image, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT)) }

  override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
    // Lay out directly in the first RN layout pass; do not post another task.
    image.layout(0, 0, right - left, bottom - top)
  }
}
