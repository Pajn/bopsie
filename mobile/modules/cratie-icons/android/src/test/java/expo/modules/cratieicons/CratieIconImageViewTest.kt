package expo.modules.cratieicons

import android.graphics.Color
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
@LooperMode(LooperMode.Mode.PAUSED)
class CratieIconImageViewTest {
  @Test fun everyGeneratedVectorIsAssignedBeforeAnyQueuedTaskRuns() {
    val image = CratieIconImageView(RuntimeEnvironment.getApplication())
    val icons = R.drawable::class.java.fields.filter { it.name.startsWith("cratie_icon_") }
    assertEquals(287, icons.size)
    for (icon in icons) {
      val name = icon.name.removePrefix("cratie_icon_").replace('_', '-')
      image.setIcon(name)
      assertNotNull("No synchronous drawable for $name", image.drawable)
      assertTrue(image.drawable.intrinsicWidth > 0)
      assertTrue(image.drawable.intrinsicHeight > 0)
    }
  }

  @Test fun viewsDoNotShareMutableDrawablesOrTint() {
    val context = RuntimeEnvironment.getApplication()
    val first = CratieIconImageView(context)
    val second = CratieIconImageView(context)
    first.setIcon("home")
    second.setIcon("home")
    first.setIconTint(Color.RED)
    second.setIconTint(Color.BLUE)
    assertNotSame(first.drawable, second.drawable)
    assertEquals(Color.RED, first.imageTintList?.defaultColor)
    assertEquals(Color.BLUE, second.imageTintList?.defaultColor)
  }

  @Test fun changingNamesReplacesTheDrawableAndUnknownNamesClearIt() {
    val image = CratieIconImageView(RuntimeEnvironment.getApplication())
    image.setIcon("play")
    val play = image.drawable
    image.setIcon("pause")
    assertNotNull(image.drawable)
    assertNotSame(play, image.drawable)
    image.setIcon("not-a-real-icon")
    assertNull(image.drawable)
  }
}
