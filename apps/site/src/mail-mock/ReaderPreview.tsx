import type { MessagePreview } from './types'

export function ReaderPreview({ message }: { message: MessagePreview | null }) {
  return (
    <section className="mock-reader" aria-labelledby="hero-title">
      <SloganTitle id="hero-title" />
      {message ? (
        <div className="reader-message" aria-live="polite">
          <h2>{message.subject}</h2>
          <div
            className="reader-message-body"
            dangerouslySetInnerHTML={{ __html: message.html }}
          />
        </div>
      ) : (
        <div className="reader-message zero-reader" aria-live="polite">
          <h2>No mail here.</h2>
          <p>For once, the mailbox is quieter than the keyboard.</p>
        </div>
      )}
    </section>
  )
}

function SloganTitle({ id }: { id?: string }) {
  return (
    <h1 className="slogan" id={id}>
      <span>Your email,</span>
      <span>
        delivered at Post
        <span className="letter-h">h</span>
        <span className="letter-a">a</span>
        <span className="letter-s">s</span>
        <span className="letter-t">t</span>
        <span className="letter-e">e</span>
      </span>
    </h1>
  )
}
