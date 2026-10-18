import ExpoModulesCore
import UIKit

public final class BopsieIconsModule: Module {
  public func definition() -> ModuleDefinition {
    Name("BopsieIcons")
    View(BopsieIconView.self) {
      Prop("name") { (view: BopsieIconView, name: String) in view.setIcon(name) }
      Prop("tint") { (view: BopsieIconView, color: UIColor) in view.image.tintColor = color }
    }
  }
}

final class BopsieIconView: ExpoView {
  let image = UIImageView()
  private static let iconsBundle: Bundle? = {
    let moduleBundle = Bundle(for: BopsieIconsModule.self)
    return moduleBundle.url(forResource: "BopsieIcons", withExtension: "bundle").flatMap(Bundle.init(url:))
  }()

  required init(appContext: AppContext? = nil) {
    super.init(appContext: appContext)
    image.contentMode = .scaleAspectFit
    image.isAccessibilityElement = false
    addSubview(image)
  }

  func setIcon(_ name: String) {
    let resourceName = "bopsie_icon_" + name.replacingOccurrences(of: "-", with: "_")
    // UIImage's named asset lookup is synchronous and uses UIKit's image cache.
    image.image = Self.iconsBundle.flatMap {
      UIImage(named: resourceName, in: $0, compatibleWith: nil)?.withRenderingMode(.alwaysTemplate)
    }
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    image.frame = bounds
  }
}
