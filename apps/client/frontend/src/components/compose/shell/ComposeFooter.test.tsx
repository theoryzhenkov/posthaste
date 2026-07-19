import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import { ComposeFooter } from './ComposeFooter'

function renderFooter(): string {
  return renderToStaticMarkup(
    <ComposeFooter
      errorMessage={null}
      fieldsDisabled={false}
      fileInputRef={{ current: null }}
      isReadingAttachments={false}
      isSending={false}
      statusLabel="Ready"
      onAttachFiles={() => {}}
      onClose={() => {}}
      onSubmit={() => {}}
    />,
  )
}

describe('ComposeFooter', () => {
  test('offers a plain Send button with no schedule expander', () => {
    const markup = renderFooter()
    expect(markup).toContain('Send')
    // Scheduled send is not offered in the UI: no split-button expander, no
    // "Send later" affordance.
    expect(markup).not.toContain('Send later')
    expect(markup).not.toContain('Schedule')
  })
})
