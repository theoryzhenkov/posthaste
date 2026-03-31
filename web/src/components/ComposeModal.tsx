import { useCallback, useEffect, useRef, useState } from "react";
import { fetchIdentity, previewMarkdown, sendEmail } from "../api/client";
import type { Recipient } from "../api/types";

interface ComposeModalProps {
  onClose: () => void;
  initialTo?: Recipient[];
  initialCc?: Recipient[];
  initialSubject?: string;
  initialBody?: string;
  inReplyTo?: string | null;
  references?: string | null;
}

function formatRecipients(recipients: Recipient[]): string {
  return recipients.map((r) => (r.name ? `${r.name} <${r.email}>` : r.email)).join(", ");
}

function parseRecipients(input: string): Recipient[] {
  if (input.trim() === "") return [];
  return input
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
    .map((s) => {
      const match = s.match(/^(.+?)\s*<(.+?)>$/);
      if (match) {
        return { name: match[1].trim(), email: match[2].trim() };
      }
      return { name: null, email: s };
    });
}

export function ComposeModal({
  onClose,
  initialTo = [],
  initialCc = [],
  initialSubject = "",
  initialBody = "",
  inReplyTo = null,
  references = null,
}: ComposeModalProps) {
  const [fromDisplay, setFromDisplay] = useState("Loading...");
  const [to, setTo] = useState(() => formatRecipients(initialTo));
  const [cc, setCc] = useState(() => formatRecipients(initialCc));
  const [bcc, setBcc] = useState("");
  const [showCcBcc, setShowCcBcc] = useState(() => initialCc.length > 0);
  const [subject, setSubject] = useState(initialSubject);
  const [body, setBody] = useState(initialBody);
  const [previewHtml, setPreviewHtml] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const debounceTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Fetch identity on mount
  useEffect(() => {
    fetchIdentity()
      .then((identity) => setFromDisplay(`${identity.name} <${identity.email}>`))
      .catch(() => setFromDisplay("(could not load identity)"));
  }, []);

  // Debounced preview
  useEffect(() => {
    if (debounceTimer.current) clearTimeout(debounceTimer.current);
    if (body.trim() === "") {
      setPreviewHtml("");
      return;
    }
    debounceTimer.current = setTimeout(() => {
      previewMarkdown(body)
        .then((res) => setPreviewHtml(res.html))
        .catch(() => setPreviewHtml("<p><em>Preview failed</em></p>"));
    }, 500);
    return () => {
      if (debounceTimer.current) clearTimeout(debounceTimer.current);
    };
  }, [body]);

  // Escape key closes modal
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  const handleSend = useCallback(async () => {
    setError(null);
    const toRecipients = parseRecipients(to);
    if (toRecipients.length === 0) {
      setError("At least one recipient is required in the To field.");
      return;
    }

    setIsSending(true);
    try {
      await sendEmail({
        to: toRecipients,
        cc: parseRecipients(cc),
        bcc: parseRecipients(bcc),
        subject,
        body,
        inReplyTo,
        references,
      });
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to send email");
    } finally {
      setIsSending(false);
    }
  }, [to, cc, bcc, subject, body, inReplyTo, references, onClose]);

  return (
    <div className="compose-overlay" onClick={onClose}>
      <div className="compose-modal" onClick={(e) => e.stopPropagation()}>
        <div className="compose-header">
          <button className="compose-close-btn" onClick={onClose} type="button">
            &times; Close
          </button>
          <button
            className="compose-send-btn"
            onClick={handleSend}
            disabled={isSending}
            type="button"
          >
            {isSending ? "Sending..." : "Send"}
          </button>
        </div>

        {error && <div className="compose-error">{error}</div>}

        <div className="compose-fields">
          <label>
            <span className="compose-field-label">From</span>
            <input type="text" value={fromDisplay} readOnly />
          </label>
          <label>
            <span className="compose-field-label">To</span>
            <input
              type="text"
              value={to}
              onChange={(e) => setTo(e.target.value)}
              placeholder="recipient@example.com"
            />
            {!showCcBcc && (
              <button
                className="compose-cc-toggle"
                onClick={() => setShowCcBcc(true)}
                type="button"
              >
                Cc/Bcc
              </button>
            )}
          </label>
          {showCcBcc && (
            <>
              <label>
                <span className="compose-field-label">Cc</span>
                <input
                  type="text"
                  value={cc}
                  onChange={(e) => setCc(e.target.value)}
                  placeholder="cc@example.com"
                />
              </label>
              <label>
                <span className="compose-field-label">Bcc</span>
                <input
                  type="text"
                  value={bcc}
                  onChange={(e) => setBcc(e.target.value)}
                  placeholder="bcc@example.com"
                />
              </label>
            </>
          )}
          <label>
            <span className="compose-field-label">Subject</span>
            <input
              type="text"
              value={subject}
              onChange={(e) => setSubject(e.target.value)}
              placeholder="Subject"
            />
          </label>
        </div>

        <div className="compose-body">
          <div className="compose-editor">
            <textarea
              value={body}
              onChange={(e) => setBody(e.target.value)}
              placeholder="Write your email in Markdown..."
            />
          </div>
          <div className="compose-preview">
            {previewHtml ? (
              <div dangerouslySetInnerHTML={{ __html: previewHtml }} />
            ) : (
              <p className="compose-preview__placeholder">
                Preview will appear here...
              </p>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
