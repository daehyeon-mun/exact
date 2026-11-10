// The page as authored, read from the views. On the Mac a hidden node's
// children wait in a view that is in no window (`Mac/ParkingMac.swift`), and
// `structuralSuperview`, `structuralSubviews` and `isWithin` read across it.
// UIKit keeps a hidden node's children under it, so there the three are the
// view tree's own, and the code both hosts share reads one name.
#if canImport(UIKit)
import UIKit

extension UIView {
    var structuralSuperview: UIView? { superview }
    var structuralSubviews: [UIView] { subviews }
    func isWithin(_ ancestor: UIView) -> Bool { isDescendant(of: ancestor) }
}
#endif
