#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import os
from dataclasses import dataclass, field
from datetime import datetime, timezone
from email.message import EmailMessage
from email.utils import format_datetime
from pathlib import Path
from typing import Literal

MailboxName = Literal[
    "Inbox",
    "Archive",
    "Drafts",
    "Sent Items",
    "Deleted Items",
    "Junk Mail",
]

UTC = timezone.utc

MAILBOX_DIRS: dict[MailboxName, str | None] = {
    "Inbox": None,
    "Archive": ".Archive",
    "Drafts": ".Drafts",
    "Sent Items": ".Sent Items",
    "Deleted Items": ".Deleted Items",
    "Junk Mail": ".Junk Mail",
}

INLINE_PNG_BYTES = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9n0l0AAAAASUVORK5CYII="
)

PDF_BYTES = (
    b"%PDF-1.4\n"
    b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
    b"2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n"
    b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 144]/Contents 4 0 R>>endobj\n"
    b"4 0 obj<</Length 44>>stream\nBT /F1 18 Tf 24 96 Td (Posthaste fixture PDF) Tj ET\nendstream\nendobj\n"
    b"xref\n0 5\n0000000000 65535 f \n0000000009 00000 n \n0000000056 00000 n \n0000000113 00000 n \n0000000200 00000 n \n"
    b"trailer<</Size 5/Root 1 0 R>>\nstartxref\n307\n%%EOF\n"
)

ZIP_BYTES = b"PK\x03\x04fixture-archive"


@dataclass(frozen=True)
class AttachmentFixture:
    filename: str
    mime_type: str
    content: bytes
    inline_cid: str | None = None


@dataclass(frozen=True)
class MessageFixture:
    mailbox: MailboxName
    subject: str
    sender: str
    to: tuple[str, ...]
    when: datetime
    message_id: str
    text_body: str
    html_body: str | None = None
    cc: tuple[str, ...] = ()
    seen: bool = True
    flagged: bool = False
    in_reply_to: str | None = None
    references: tuple[str, ...] = ()
    attachments: tuple[AttachmentFixture, ...] = field(default_factory=tuple)


def build_fixtures() -> list[MessageFixture]:
    kickoff_id = "<launch-kickoff@fixtures.posthaste.local>"
    revision_id = "<launch-revision@fixtures.posthaste.local>"
    reply_id = "<launch-reply@fixtures.posthaste.local>"
    wrap_id = "<launch-wrap@fixtures.posthaste.local>"

    design_request_id = "<design-request@fixtures.posthaste.local>"
    design_reply_id = "<design-reply@fixtures.posthaste.local>"

    return [
        MessageFixture(
            mailbox="Archive",
            subject="Monday launch checklist",
            sender="Maya Chen <maya.chen@meridian.example>",
            to=("dev@localhost",),
            when=datetime(2026, 4, 15, 9, 12, tzinfo=UTC),
            message_id=kickoff_id,
            text_body=(
                "Starting a single thread for launch coordination.\n\n"
                "We need final QA sign-off, the rollout window, and the customer note."
            ),
        ),
        MessageFixture(
            mailbox="Deleted Items",
            subject="Past due: April invoice",
            sender="Accounting <accounts@papertrail.example>",
            to=("dev@localhost",),
            when=datetime(2026, 4, 18, 6, 55, tzinfo=UTC),
            message_id="<invoice-trash@fixtures.posthaste.local>",
            seen=True,
            text_body=(
                "This reminder can stay deleted.\n\n"
                "The invoice was already paid through the portal."
            ),
            attachments=(
                AttachmentFixture(
                    filename="invoice-042026.pdf",
                    mime_type="application/pdf",
                    content=PDF_BYTES,
                ),
            ),
        ),
        MessageFixture(
            mailbox="Junk Mail",
            subject="Action required: mailbox quota exceeded",
            sender="Security Center <security-update@outlook-reset.example>",
            to=("dev@localhost",),
            when=datetime(2026, 4, 20, 3, 11, tzinfo=UTC),
            message_id="<spam-quota@fixtures.posthaste.local>",
            seen=True,
            text_body=(
                "This is obvious spam, kept only to exercise the junk mailbox UI.\n\n"
                "Do not open the attachment."
            ),
            attachments=(
                AttachmentFixture(
                    filename="quota-fix.zip",
                    mime_type="application/zip",
                    content=ZIP_BYTES,
                ),
            ),
        ),
        MessageFixture(
            mailbox="Archive",
            subject="Flight and hotel confirmation",
            sender="Travel Desk <travel@alder.example>",
            to=("dev@localhost",),
            when=datetime(2026, 4, 12, 13, 41, tzinfo=UTC),
            message_id="<travel-confirmation@fixtures.posthaste.local>",
            seen=True,
            text_body=(
                "Your itinerary for the Austin offsite is attached.\n\n"
                "Calendar invite and receipt included."
            ),
            attachments=(
                AttachmentFixture(
                    filename="offsite-itinerary.pdf",
                    mime_type="application/pdf",
                    content=PDF_BYTES,
                ),
                AttachmentFixture(
                    filename="offsite.ics",
                    mime_type="text/calendar",
                    content=(
                        b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n"
                        b"SUMMARY:Austin offsite\r\nDTSTART:20260503T150000Z\r\n"
                        b"DTEND:20260506T180000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
                    ),
                ),
            ),
        ),
        MessageFixture(
            mailbox="Inbox",
            subject="Re: Monday launch checklist",
            sender="Maya Chen <maya.chen@meridian.example>",
            to=("dev@localhost",),
            cc=("ops@meridian.example",),
            when=datetime(2026, 4, 21, 14, 5, tzinfo=UTC),
            message_id=revision_id,
            in_reply_to=kickoff_id,
            references=(kickoff_id,),
            seen=True,
            flagged=True,
            text_body=(
                "Attached the revised launch plan.\n\n"
                "Please sanity-check the customer messaging before tomorrow."
            ),
            attachments=(
                AttachmentFixture(
                    filename="launch-plan-v2.pdf",
                    mime_type="application/pdf",
                    content=PDF_BYTES,
                ),
            ),
        ),
        MessageFixture(
            mailbox="Sent Items",
            subject="Re: Monday launch checklist",
            sender="Dev Account <dev@localhost>",
            to=("Maya Chen <maya.chen@meridian.example>",),
            cc=("ops@meridian.example",),
            when=datetime(2026, 4, 21, 15, 2, tzinfo=UTC),
            message_id=reply_id,
            in_reply_to=revision_id,
            references=(kickoff_id, revision_id),
            seen=True,
            text_body=(
                "Copy looks good.\n\n"
                "I tightened the rollout note and queued the status banner update."
            ),
            html_body=(
                "<p>Copy looks good.</p>"
                "<p>I tightened the rollout note and queued the status banner update.</p>"
            ),
        ),
        MessageFixture(
            mailbox="Drafts",
            subject="Vendor contract questions",
            sender="Dev Account <dev@localhost>",
            to=("sales@vendor.example",),
            when=datetime(2026, 4, 22, 16, 9, tzinfo=UTC),
            message_id="<vendor-draft@fixtures.posthaste.local>",
            seen=True,
            text_body=(
                "Need to ask about termination language and support response times.\n\n"
                "Holding this draft until legal replies."
            ),
        ),
        MessageFixture(
            mailbox="Sent Items",
            subject="Design review follow-up",
            sender="Dev Account <dev@localhost>",
            to=("Priya Raman <priya.raman@aperture.example>",),
            when=datetime(2026, 4, 22, 17, 18, tzinfo=UTC),
            message_id=design_request_id,
            seen=True,
            text_body=(
                "Sending the final spacing tweaks for the settings sheet.\n\n"
                "Let me know if you want one more pass before handoff."
            ),
        ),
        MessageFixture(
            mailbox="Inbox",
            subject="Domain renewal confirmed",
            sender="Billing <billing@registrar.example>",
            to=("dev@localhost",),
            when=datetime(2026, 4, 22, 11, 6, tzinfo=UTC),
            message_id="<domain-renewal@fixtures.posthaste.local>",
            seen=True,
            text_body=(
                "Renewal completed for posthaste.dev.\n\n"
                "The receipt is available in the billing portal."
            ),
            html_body=(
                "<p>Renewal completed for <strong>posthaste.dev</strong>.</p>"
                "<p>The receipt is available in the billing portal.</p>"
            ),
        ),
        MessageFixture(
            mailbox="Inbox",
            subject="Customer escalation digest",
            sender="Support Ops <support@relay.example>",
            to=("dev@localhost",),
            when=datetime(2026, 4, 23, 7, 32, tzinfo=UTC),
            message_id="<support-digest@fixtures.posthaste.local>",
            seen=False,
            text_body=(
                "Three customer escalations need triage today.\n\n"
                "The attached CSV has account IDs, owners, and current severity."
            ),
            attachments=(
                AttachmentFixture(
                    filename="escalations.csv",
                    mime_type="text/csv",
                    content=(
                        b"account_id,owner,severity\n"
                        b"acct_481,ana,high\n"
                        b"acct_772,joel,medium\n"
                        b"acct_190,casey,high\n"
                    ),
                ),
            ),
        ),
        MessageFixture(
            mailbox="Inbox",
            subject="Re: Monday launch checklist",
            sender="Nadia Ortiz <nadia.ortiz@meridian.example>",
            to=("dev@localhost",),
            cc=("Maya Chen <maya.chen@meridian.example>",),
            when=datetime(2026, 4, 23, 8, 48, tzinfo=UTC),
            message_id=wrap_id,
            in_reply_to=reply_id,
            references=(kickoff_id, revision_id, reply_id),
            seen=False,
            text_body=(
                "Attaching the burn-up snapshot from this morning.\n\n"
                "We are clear to open the feature flag after support signs off."
            ),
            html_body=(
                "<p>Attaching the burn-up snapshot from this morning.</p>"
                '<p><img src="cid:launch-burnup@fixtures.posthaste.local" alt="Launch burn-up" /></p>'
                "<p>We are clear to open the feature flag after support signs off.</p>"
            ),
            attachments=(
                AttachmentFixture(
                    filename="launch-burnup.png",
                    mime_type="image/png",
                    content=INLINE_PNG_BYTES,
                    inline_cid="launch-burnup@fixtures.posthaste.local",
                ),
            ),
        ),
        MessageFixture(
            mailbox="Drafts",
            subject="Notes for leadership sync",
            sender="Dev Account <dev@localhost>",
            to=("leadership@localhost",),
            when=datetime(2026, 4, 23, 9, 5, tzinfo=UTC),
            message_id="<leadership-draft@fixtures.posthaste.local>",
            seen=True,
            text_body=(
                "Drafting the talking points for the leadership sync.\n\n"
                "Still need to tighten the rollout risks section."
            ),
            attachments=(
                AttachmentFixture(
                    filename="leadership-notes.md",
                    mime_type="text/markdown",
                    content=(
                        b"# Leadership sync\n\n"
                        b"- Launch status\n"
                        b"- Support load\n"
                        b"- Migration risk\n"
                    ),
                ),
            ),
        ),
        MessageFixture(
            mailbox="Inbox",
            subject="Re: Design review follow-up",
            sender="Priya Raman <priya.raman@aperture.example>",
            to=("dev@localhost",),
            when=datetime(2026, 4, 23, 10, 14, tzinfo=UTC),
            message_id=design_reply_id,
            in_reply_to=design_request_id,
            references=(design_request_id,),
            seen=False,
            text_body=(
                "Looks good from my side.\n\n"
                "Only follow-up is a tighter empty state on the smart mailbox panel."
            ),
        ),
    ]


def ensure_maildir(root: Path) -> None:
    for mailbox_dir in MAILBOX_DIRS.values():
        mailbox_root = root if mailbox_dir is None else root / mailbox_dir
        for child in ("cur", "new", "tmp"):
            (mailbox_root / child).mkdir(parents=True, exist_ok=True)


def split_mime_type(mime_type: str) -> tuple[str, str]:
    maintype, subtype = mime_type.split("/", maxsplit=1)
    return maintype, subtype


def build_message(fixture: MessageFixture) -> EmailMessage:
    message = EmailMessage()
    message["From"] = fixture.sender
    message["To"] = ", ".join(fixture.to)
    if fixture.cc:
        message["Cc"] = ", ".join(fixture.cc)
    message["Subject"] = fixture.subject
    message["Date"] = format_datetime(fixture.when)
    message["Message-ID"] = fixture.message_id
    if fixture.in_reply_to:
        message["In-Reply-To"] = fixture.in_reply_to
    if fixture.references:
        message["References"] = " ".join(fixture.references)
    message["X-Posthaste-Fixture"] = "stalwart-dev-seed"
    message.set_content(fixture.text_body)

    if fixture.html_body is not None:
        message.add_alternative(fixture.html_body, subtype="html")

    html_part = message.get_body(preferencelist=("html",))
    for attachment in fixture.attachments:
        maintype, subtype = split_mime_type(attachment.mime_type)
        if attachment.inline_cid:
            if html_part is None:
                raise ValueError(
                    f"inline attachment {attachment.filename} requires an HTML body"
                )
            html_part.add_related(
                attachment.content,
                maintype=maintype,
                subtype=subtype,
                cid=f"<{attachment.inline_cid}>",
                filename=attachment.filename,
                disposition="inline",
            )
            continue
        message.add_attachment(
            attachment.content,
            maintype=maintype,
            subtype=subtype,
            filename=attachment.filename,
        )

    return message


def message_path(root: Path, fixture: MessageFixture, index: int) -> Path:
    mailbox_dir = MAILBOX_DIRS[fixture.mailbox]
    mailbox_root = root if mailbox_dir is None else root / mailbox_dir

    if fixture.seen or fixture.flagged:
        flags = "".join(
            flag
            for enabled, flag in ((fixture.flagged, "F"), (fixture.seen, "S"))
            if enabled
        )
        return mailbox_root / "cur" / f"{int(fixture.when.timestamp())}.{index:02d}:2,{flags}"

    return mailbox_root / "new" / f"{int(fixture.when.timestamp())}.{index:02d}"


def write_fixtures(root: Path) -> None:
    ensure_maildir(root)
    fixtures = sorted(build_fixtures(), key=lambda fixture: fixture.when)
    for index, fixture in enumerate(fixtures, start=1):
        path = message_path(root, fixture, index)
        path.write_bytes(build_message(fixture).as_bytes())
        epoch = fixture.when.timestamp()
        os.utime(path, (epoch, epoch))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate deterministic Maildir++ fixtures for Stalwart dev seeding."
    )
    parser.add_argument("output_dir", type=Path, help="Empty directory to populate")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output_dir: Path = args.output_dir
    output_dir.mkdir(parents=True, exist_ok=True)
    if any(output_dir.iterdir()):
        raise SystemExit(f"output directory must be empty: {output_dir}")
    write_fixtures(output_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
