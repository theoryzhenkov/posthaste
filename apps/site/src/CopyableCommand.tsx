import { Check, Copy } from 'lucide-react'
import { useState } from 'react'

/**
 * A shell command shown in a code block with a copy-to-clipboard button. The
 * command may span multiple lines (shell `\`-continuations); whitespace is
 * preserved (`white-space: pre`) so the copied text pastes straight into a
 * terminal. The Copy button floats top-right so it never collides with the
 * command text.
 */
export function CopyableCommand({ command }: { command: string }) {
  const [copied, setCopied] = useState(false)

  const copy = () => {
    void navigator.clipboard?.writeText(command).then(() => {
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1600)
    })
  }

  return (
    <div className="command-block">
      <pre>
        <code>{command}</code>
      </pre>
      <button
        type="button"
        className="command-copy"
        onClick={copy}
        aria-label={copied ? 'Copied' : 'Copy command'}
      >
        {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
        <span>{copied ? 'Copied' : 'Copy'}</span>
      </button>
    </div>
  )
}
