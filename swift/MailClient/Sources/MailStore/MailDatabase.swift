import Foundation
import GRDB
import MailBridge

// MARK: - Database Records

public struct MailboxRecord: Codable, FetchableRecord, PersistableRecord, Identifiable, Sendable {
    public static let databaseTableName = "mailbox"

    public let id: String
    public let accountId: String
    public let name: String
    public let parentId: String?
    public let role: String?
    public let sortOrder: Int
    public let totalEmails: Int
    public let unreadEmails: Int
}

public struct EmailRecord: Codable, FetchableRecord, PersistableRecord, Identifiable, Sendable {
    public static let databaseTableName = "email"

    public let id: String
    public let accountId: String
    public let threadId: String
    public let subject: String?
    public let fromName: String?
    public let fromEmail: String?
    public let preview: String?
    public let receivedAt: Date
    public let hasAttachment: Bool
    public let isRead: Bool
    public let isFlagged: Bool
    public let mailboxId: String
}

// MARK: - Database

public final class MailDatabase: Sendable {
    public let dbQueue: DatabaseQueue

    public init() throws {
        let url = try FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent("MailClient", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)

        let dbPath = url.appendingPathComponent("mail.sqlite").path
        dbQueue = try DatabaseQueue(path: dbPath)
        try migrate()
    }

    private func migrate() throws {
        var migrator = DatabaseMigrator()

        migrator.registerMigration("v1") { db in
            try db.create(table: "mailbox", ifNotExists: true) { t in
                t.column("id", .text).notNull()
                t.column("accountId", .text).notNull()
                t.column("name", .text).notNull()
                t.column("parentId", .text)
                t.column("role", .text)
                t.column("sortOrder", .integer).notNull().defaults(to: 0)
                t.column("totalEmails", .integer).notNull().defaults(to: 0)
                t.column("unreadEmails", .integer).notNull().defaults(to: 0)
                t.primaryKey(["accountId", "id"])
            }

            try db.create(table: "email", ifNotExists: true) { t in
                t.column("id", .text).notNull()
                t.column("accountId", .text).notNull()
                t.column("threadId", .text).notNull()
                t.column("subject", .text)
                t.column("fromName", .text)
                t.column("fromEmail", .text)
                t.column("preview", .text)
                t.column("receivedAt", .datetime).notNull()
                t.column("hasAttachment", .boolean).notNull().defaults(to: false)
                t.column("isRead", .boolean).notNull().defaults(to: true)
                t.column("isFlagged", .boolean).notNull().defaults(to: false)
                t.column("mailboxId", .text).notNull()
                t.primaryKey(["accountId", "id"])
            }

            try db.create(indexOn: "email", columns: ["accountId", "mailboxId"])
            try db.create(indexOn: "email", columns: ["accountId", "threadId"])
        }

        try migrator.migrate(dbQueue)
    }

    // MARK: - Write (CacheWriter role)

    public func importFromRust(client: MailClient, accountId: String) throws {
        let mailboxes = client.getMailboxes()
        let emails = client.getAllEmails()

        try dbQueue.write { db in
            try MailboxRecord.filter(Column("accountId") == accountId).deleteAll(db)
            try EmailRecord.filter(Column("accountId") == accountId).deleteAll(db)

            for m in mailboxes {
                let record = MailboxRecord(
                    id: m.id,
                    accountId: accountId,
                    name: m.name,
                    parentId: m.parentId,
                    role: m.role,
                    sortOrder: Int(m.sortOrder),
                    totalEmails: Int(m.totalEmails),
                    unreadEmails: Int(m.unreadEmails)
                )
                try record.insert(db)
            }

            for e in emails {
                let isRead = e.keywords.contains("$seen")
                let isFlagged = e.keywords.contains("$flagged")
                let date = Date(timeIntervalSince1970: TimeInterval(e.receivedAt))

                for mailboxId in e.mailboxIds {
                    let record = EmailRecord(
                        id: e.id,
                        accountId: accountId,
                        threadId: e.threadId,
                        subject: e.subject,
                        fromName: e.fromName,
                        fromEmail: e.fromEmail,
                        preview: e.preview,
                        receivedAt: date,
                        hasAttachment: e.hasAttachment,
                        isRead: isRead,
                        isFlagged: isFlagged,
                        mailboxId: mailboxId
                    )
                    try record.insert(db)
                }
            }
        }
    }

    // MARK: - Read

    public func fetchMailboxes(accountId: String) throws -> [MailboxRecord] {
        try dbQueue.read { db in
            try MailboxRecord
                .filter(Column("accountId") == accountId)
                .order(Column("sortOrder"))
                .fetchAll(db)
        }
    }

    public func fetchEmails(accountId: String, mailboxId: String) throws -> [EmailRecord] {
        try dbQueue.read { db in
            try EmailRecord
                .filter(Column("accountId") == accountId)
                .filter(Column("mailboxId") == mailboxId)
                .order(Column("receivedAt").desc)
                .fetchAll(db)
        }
    }
}
