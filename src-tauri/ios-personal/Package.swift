// swift-tools-version:5.7
import PackageDescription

let package = Package(
    name: "DabneyHealthKit",
    platforms: [
        .iOS(.v16)
    ],
    products: [
        .library(
            name: "DabneyHealthKit",
            type: .static,
            targets: ["DabneyHealthKit"]
        )
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "DabneyHealthKit",
            dependencies: [
                .byName(name: "Tauri")
            ],
            path: "Sources"
        )
    ]
)
