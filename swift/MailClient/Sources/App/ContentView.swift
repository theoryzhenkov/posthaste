import SwiftUI
import MailStore

struct ContentView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        NavigationSplitView {
            SidebarView()
        } content: {
            MessageListView()
        } detail: {
            MessageDetailView()
        }
    }
}

// MARK: - Sidebar

struct SidebarView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        List(state.mailboxes, selection: Bindable(state).selectedMailboxId) { mailbox in
            NavigationLink(value: mailbox.id) {
                HStack {
                    Image(systemName: iconForRole(mailbox.role))
                        .frame(width: 20)
                    Text(mailbox.name)
                    Spacer()
                    if mailbox.unreadEmails > 0 {
                        Text("\(mailbox.unreadEmails)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(.quaternary, in: Capsule())
                    }
                }
            }
        }
        .navigationTitle("Mailboxes")
    }

    private func iconForRole(_ role: String?) -> String {
        switch role {
        case "inbox": "tray"
        case "sent": "paperplane"
        case "drafts": "doc"
        case "trash": "trash"
        case "archive": "archivebox"
        default: "folder"
        }
    }
}

// MARK: - Message List

struct MessageListView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        Group {
            if state.emails.isEmpty {
                ContentUnavailableView(
                    "No Messages",
                    systemImage: "tray",
                    description: Text("Select a mailbox to view messages.")
                )
            } else {
                List(state.emails, selection: Bindable(state).selectedEmailId) { email in
                    NavigationLink(value: email.id) {
                        MessageRowView(email: email)
                    }
                }
            }
        }
        .navigationTitle(selectedMailboxName)
    }

    private var selectedMailboxName: String {
        state.mailboxes.first(where: { $0.id == state.selectedMailboxId })?.name ?? "Mail"
    }
}

struct MessageRowView: View {
    let email: EmailRecord

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                if !email.isRead {
                    Circle()
                        .fill(.blue)
                        .frame(width: 8, height: 8)
                }
                Text(email.fromName ?? email.fromEmail ?? "Unknown")
                    .font(.headline)
                    .fontWeight(email.isRead ? .regular : .bold)
                    .lineLimit(1)
                Spacer()
                if email.hasAttachment {
                    Image(systemName: "paperclip")
                        .foregroundStyle(.secondary)
                }
                if email.isFlagged {
                    Image(systemName: "flag.fill")
                        .foregroundStyle(.orange)
                        .font(.caption)
                }
                Text(email.receivedAt, style: .relative)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Text(email.subject ?? "(no subject)")
                .font(.subheadline)
                .foregroundStyle(email.isRead ? .secondary : .primary)
                .lineLimit(1)
            if let preview = email.preview {
                Text(preview)
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: - Message Detail

struct MessageDetailView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        if let emailId = state.selectedEmailId,
           let email = state.emails.first(where: { $0.id == emailId }) {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    // Header
                    VStack(alignment: .leading, spacing: 8) {
                        Text(email.subject ?? "(no subject)")
                            .font(.title2)
                            .fontWeight(.semibold)

                        HStack {
                            Text(email.fromName ?? "Unknown")
                                .fontWeight(.medium)
                            if let addr = email.fromEmail {
                                Text("<\(addr)>")
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            Text(email.receivedAt, style: .date)
                                .foregroundStyle(.secondary)
                            Text(email.receivedAt, style: .time)
                                .foregroundStyle(.secondary)
                        }
                        .font(.subheadline)
                    }
                    .padding()
                    .background(.background.secondary)
                    .clipShape(RoundedRectangle(cornerRadius: 8))

                    // Body placeholder
                    if let preview = email.preview {
                        Text(preview)
                            .font(.body)
                            .padding()
                    }

                    Spacer()
                }
                .padding()
            }
        } else {
            ContentUnavailableView(
                "No Message Selected",
                systemImage: "envelope",
                description: Text("Select a message to read it.")
            )
        }
    }
}
