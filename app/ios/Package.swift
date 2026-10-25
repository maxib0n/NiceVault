// swift-tools-version: 6.0
// NiceVault for iOS, built from Linux with xtool (scripts/build-ios.sh).
//
// The app is the Rust library app/src-tauri (Tauri 2) compiled as libnicevault_app.a for
// aarch64-apple-ios; this package only provides `main` (which calls Rust's `start_app`) and
// the Swift halves Tauri needs on iOS. Those are not built by cargo on a Linux host (tauri's
// build.rs only runs swift-rs on macOS), so they are SwiftPM dependencies here instead:
// build-ios.sh copies them out of the cargo registry into Packages/ (gitignored):
//   Packages/.tauri/tauri-api              tauri's mobile/ios-api    (`Tauri`: log_stdout, on_webview_created, ...)
//   Packages/tauri-plugin-dialog           tauri-plugin-dialog/ios   (init_plugin_dialog)
//   Packages/tauri-plugin-clipboard-manager tauri-plugin-clipboard-manager/ios (init_plugin_clipboard)
// and links Libs/libnicevault_app.a (a symlink to the cargo output).

import PackageDescription

let libs = Context.packageDirectory + "/Libs"

let package = Package(
    name: "NiceVault",
    platforms: [
        .iOS(.v15), // = min in scripts/build-ios.sh
    ],
    products: [
        // An xtool project contains exactly one library product: the app.
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
            // NvPlugin.swift follows Tauri's plugin model (main-thread UIKit, callbacks from
            // NSItemProvider queues), written for the Swift 5 concurrency rules
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                // what `cargo rustc --crate-type staticlib -- --print native-static-libs` reports
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
