package expo.modules.cratieicons

import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.ColorPropConverter
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class CratieIconsModule : Module() {
  override fun definition() = ModuleDefinition {
    Name("CratieIcons")
    View(CratieIconView::class) {
      Prop("name") { view: CratieIconView, name: String -> view.image.setIcon(name) }
      Prop("tint") { view: CratieIconView, color: Any? ->
        @Suppress("UNCHECKED_CAST")
        val value = if (color is Map<*, *>) Arguments.makeNativeMap(color as Map<String, Any>) else color
        view.image.setIconTint(ColorPropConverter.getColor(value, view.context) ?: android.graphics.Color.BLACK)
      }
    }
  }
}
