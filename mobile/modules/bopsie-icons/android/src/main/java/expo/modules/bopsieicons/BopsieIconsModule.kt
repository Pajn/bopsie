package expo.modules.bopsieicons

import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.ColorPropConverter
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class BopsieIconsModule : Module() {
  override fun definition() = ModuleDefinition {
    Name("BopsieIcons")
    View(BopsieIconView::class) {
      Prop("name") { view: BopsieIconView, name: String -> view.image.setIcon(name) }
      Prop("tint") { view: BopsieIconView, color: Any? ->
        @Suppress("UNCHECKED_CAST")
        val value = if (color is Map<*, *>) Arguments.makeNativeMap(color as Map<String, Any>) else color
        view.image.setIconTint(ColorPropConverter.getColor(value, view.context) ?: android.graphics.Color.BLACK)
      }
    }
  }
}
