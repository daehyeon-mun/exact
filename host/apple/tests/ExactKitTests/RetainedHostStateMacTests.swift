#if os(macOS)
import AppKit
import XCTest
@testable import ExactKit

final class RetainedHostStateMacTests: XCTestCase {
    func testDetachedLiveChildrenLeaveTheHierarchyAndKeepSessionIdentity() throws {
        _ = NSApplication.shared
        let first = Presenter(), second = Presenter()
        let batch = wireBatch([
            ["op": "create", "id": 1, "kind": "view"],
            ["op": "create", "id": 2, "kind": "input", "props": ["value": "draft"]],
            ["op": "children", "id": 1, "ids": [2]],
            ["op": "roots", "ids": [1]],
        ])
        first.apply(batch); second.apply(batch)
        let a = try XCTUnwrap(first.views[2]), b = try XCTUnwrap(second.views[2])
        first.apply(wireBatch([["op": "children", "id": 1, "ids": []]]))
        XCTAssertNil(a.superview, "omitted live nodes must not remain among authored children")
        XCTAssertFalse(a.isHidden, "detachment must not rewrite authored visibility")
        XCTAssertTrue(b.superview === second.views[1]?.container)
        first.apply(wireBatch([["op": "children", "id": 1, "ids": [2]]]))
        XCTAssertTrue(first.views[2] === a)
        XCTAssertTrue(a.superview === first.views[1]?.container)
        XCTAssertEqual(a.field?.stringValue, "draft")
    }

    func testReleaseRetiresOnlyThatPresentersRollback() {
        let first = Presenter(), second = Presenter()
        let a = NodeView(id: 7, kind: "text-input", presenter: first)
        let b = NodeView(id: 7, kind: "text-input", presenter: second)
        first.views[7] = a; second.views[7] = b
        let before = (value: "", selection: NSRange(location: 0, length: 0))
        first.echoRollbacks[1] = PassiveInput.Rollback(node: a, before: before, after: "a")
        second.echoRollbacks[1] = PassiveInput.Rollback(node: b, before: before, after: "b")
        first.release(7, forget: true)
        XCTAssertTrue(first.echoRollbacks.isEmpty)
        XCTAssertTrue(second.echoRollbacks[1]?.node === b)
        second.reset()
        XCTAssertTrue(second.echoRollbacks.isEmpty)
    }
}
#endif
