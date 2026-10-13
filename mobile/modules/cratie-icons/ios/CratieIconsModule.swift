import ExpoModulesCore
import UIKit

public final class CratieIconsModule: Module {
  public func definition() -> ModuleDefinition {
    Name("CratieIcons")
    View(CratieIconView.self) {
      Prop("name") { (view: CratieIconView, name: String) in view.setIcon(name) }
      Prop("tint") { (view: CratieIconView, color: UIColor) in view.image.tintColor = color }
    }
  }
}

final class CratieIconView: ExpoView {
  let image = UIImageView()
  private static let iconsBundle: Bundle? = {
    let moduleBundle = Bundle(for: CratieIconsModule.self)
    return moduleBundle.url(forResource: "CratieIcons", withExtension: "bundle").flatMap(Bundle.init(url:))
  }()

  required init(appContext: AppContext? = nil) {
    super.init(appContext: appContext)
    image.contentMode = .scaleAspectFit
    image.isAccessibilityElement = false
    addSubview(image)
  }

  func setIcon(_ name: String) {
    let resourceName = "cratie_icon_" + name.replacingOccurrences(of: "-", with: "_")
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
