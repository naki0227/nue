// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "NueKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [.library(name: "NueKit", targets: ["NueKit"])],
    targets: [
        .target(name: "NueKit"),
        .testTarget(name: "NueKitTests", dependencies: ["NueKit"], exclude: ["Fixtures"]),
    ]
)
