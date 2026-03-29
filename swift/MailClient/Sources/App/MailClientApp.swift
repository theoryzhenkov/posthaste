import SwiftUI
import MailBridge
import MailStore

@main
struct MailClientApp: App {
    @State private var appState = AppState()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(appState)
        }
        .windowStyle(.automatic)
        .defaultSize(width: 1100, height: 700)
    }
}

@MainActor
@Observable
final class AppState {
    let database: MailDatabase
    let rustClient: MailClient

    var mailboxes: [MailboxRecord] = []
    var emails: [EmailRecord] = []
    var selectedMailboxId: String? {
        didSet { loadEmails() }
    }
    var selectedEmailId: String?

    private let accountId = "mock-account-1"

    init() {
        do {
            self.database = try MailDatabase()
            self.rustClient = MailClient()

            // Import mock data from Rust into GRDB
            try database.importFromRust(client: rustClient, accountId: accountId)

            // Load mailboxes
            self.mailboxes = try database.fetchMailboxes(accountId: accountId)

            // Auto-select inbox
            if let inbox = mailboxes.first(where: { $0.role == "inbox" }) {
                self.selectedMailboxId = inbox.id
                self.emails = try database.fetchEmails(accountId: accountId, mailboxId: inbox.id)
            }
        } catch {
            fatalError("Failed to initialize: \(error)")
        }
    }

    private func loadEmails() {
        guard let mailboxId = selectedMailboxId else {
            emails = []
            return
        }
        do {
            emails = try database.fetchEmails(accountId: accountId, mailboxId: mailboxId)
            selectedEmailId = nil
        } catch {
            print("Failed to load emails: \(error)")
        }
    }
}
