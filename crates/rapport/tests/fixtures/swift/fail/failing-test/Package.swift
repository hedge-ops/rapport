// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "RapportFixture",
    targets: [
        .target(name: "RapportFixture"),
        .testTarget(name: "RapportFixtureTests", dependencies: ["RapportFixture"]),
    ]
)
