import PackageDescription

let libs = Context.packageDirectory + "/Libs"

let package = Package(
    name: "NiceVault",
    platforms: [
        .iOS(.v15), // = min in scripts/build-ios.sh
    ],
    products: [
        .library(name: "NiceVault", targets: ["NiceVault"]),
    ],
    dependencies: [
        .package(name: "Tauri", path: "Packages/.tauri/tauri-api"),
        .package(path: "Packages/tauri-plugin-dialog"),
        .package(path: "Packages/tauri-plugin-clipboard-manager"),
    ],
    targets: [
        .target(
            name: "NiceVault",
            dependencies: [
                .product(name: "Tauri", package: "Tauri"),
                .product(name: "tauri-plugin-dialog", package: "tauri-plugin-dialog"),
                .product(name: "tauri-plugin-clipboard-manager", package: "tauri-plugin-clipboard-manager"),
            ],
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                .unsafeFlags(["-L", libs, "-lnicevault_app"]),
                .linkedFramework("Security"),
                .linkedFramework("WebKit"),
                .linkedFramework("Photos"),
                .linkedFramework("UIKit"),
                .linkedFramework("UserNotifications"),
                .linkedFramework("CoreData"),
                .linkedFramework("CoreLocation"),
                .linkedFramework("CoreText"),
                .linkedFramework("CoreImage"),
                .linkedFramework("CloudKit"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("ImageIO"), // NvThumb.swift
                .linkedFramework("Foundation"),
                .linkedFramework("CoreFoundation"),
                .linkedLibrary("objc"),
                .linkedLibrary("iconv"),
                .linkedLibrary("c++"),
            ]
        ),
    ]
)
