import SwiftUI

@main
struct KittenTTSApp: App {
    @StateObject private var engine = TTSEngine()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(engine)
        }
    }
}
