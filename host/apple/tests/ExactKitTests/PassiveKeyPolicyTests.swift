import XCTest
@testable import ExactKit

final class PassiveKeyPolicyTests: XCTestCase {
    func testPassiveKeysRequireAnExactTopLevelString() {
        for dataset in [
            nil, "", "not JSON", "[]", "{}", "{\"keys-passive\":true}",
            "{\"keys-passive\":\"no\"}", "{\"nested\":{\"keys-passive\":\"yes\"}}",
            "{\"label\":\"\\\"keys-passive\\\":\\\"yes\\\"\"}",
        ] {
            XCTAssertFalse(PassiveKeyPolicy.declaredPassive(dataset), "\(dataset ?? "nil")")
        }
        XCTAssertTrue(PassiveKeyPolicy.declaredPassive("{ \"keys-passive\": \"yes\" }"))
        XCTAssertTrue(PassiveKeyPolicy.declaredPassive("{\"keys-passive\":\"yes\",\"label\":\"editor\"}"))
    }

    func testControlAndFunctionCharactersCannotEcho() {
        for key in ["", "Enter", "\n", "\t", "\u{7F}", "\u{F700}"] {
            XCTAssertFalse(PassiveKeyPolicy.printable(key))
        }
        for key in ["a", " ", "å", "👩🏽‍💻"] { XCTAssertTrue(PassiveKeyPolicy.printable(key)) }
    }

    func testCommandModifiersCannotBecomePrintableEchoes() {
        for chord in ["Control+a", "Meta+a", "Shift+Control+Alt+Meta+a"] {
            XCTAssertTrue(PassiveKeyPolicy.keyName(chord).command)
        }
        XCTAssertEqual(PassiveKeyPolicy.keyName("Shift+Alt+Å").name, "Å")
        XCTAssertFalse(PassiveKeyPolicy.keyName("Alt+å").command)
        XCTAssertEqual(PassiveKeyPolicy.keyName("Enter").name, "Enter")
    }
}
