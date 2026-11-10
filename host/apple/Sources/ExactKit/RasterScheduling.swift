import Foundation

enum RasterScheduling {
    /// A scroll step refreshes the text bands only after `refreshTravel` points of travel, and then hands every
    /// paragraph within reach to the raster workers, one per performance core, so rows arrive with pixels.

    static let refreshTravel: CGFloat = 400
    static let rasterWorkers: Int = {
        #if os(macOS)
        var n: Int32 = 0, size = MemoryLayout<Int32>.size
        return sysctlbyname("hw.perflevel0.physicalcpu", &n, &size, nil, 0) == 0 && n > 0 ? Int(n) : 4
        #else
        return 2
        #endif
    }()
}
