// swift-tools-version: 6.0
import PackageDescription

let rustLibPath = "../../crates/target/release"

let package = Package(
    name: "MailClient",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "MailClient", targets: ["App"]),
    ],
    dependencies: [
        .package(url: "https://github.com/groue/GRDB.swift.git", from: "7.0.0"),
    ],
    targets: [
        // C FFI headers for the Rust static library
        .target(
            name: "CMailBridge",
            path: "Sources/CMailBridge",
            publicHeadersPath: "include"
        ),
        // Swift wrapper around the generated UniFFI bindings
        .target(
            name: "MailBridge",
            dependencies: ["CMailBridge"],
            path: "Sources/MailBridge",
            exclude: ["Generated"],
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                .unsafeFlags([
                    "-L\(rustLibPath)",
                    "-lmail_bridge",
                ]),
            ]
        ),
        // GRDB-backed local store
        .target(
            name: "MailStore",
            dependencies: [
                .product(name: "GRDB", package: "GRDB.swift"),
                "MailBridge",
            ],
            path: "Sources/MailStore",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        // Main SwiftUI app
        .executableTarget(
            name: "App",
            dependencies: ["MailBridge", "MailStore"],
            path: "Sources/App",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
