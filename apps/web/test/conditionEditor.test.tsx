import { describe, expect, it } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import type { ReactNode } from 'react'

import type {
  Mailbox,
  SmartMailboxCondition,
  SmartMailboxField,
  SmartMailboxOperator,
} from '../src/api/types'
import { ConditionEditor } from '../src/components/settings-panel/rule-group/ConditionEditor'
import {
  ConditionEditorContext,
  type ConditionEditorData,
} from '../src/components/settings-panel/rule-group/conditionEditorContext'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

// These assert WHICH widget renders per field type. The emitted-value wire
// shape (string | string[] | boolean) is covered exhaustively by the pure
// transforms in conditionValueFormat.test.ts — React's controlled-input
// onChange does not fire under fireEvent in this happy-dom setup (the codebase
// reads DOM values on blur for the same reason), so driving emission through
// the rendered inputs here would be unreliable.

const EMPTY_DATA: ConditionEditorData = {
  accountId: '',
  mailboxes: null,
  accounts: [],
}

function mkCondition(
  field: SmartMailboxField,
  operator: SmartMailboxOperator,
  value: SmartMailboxCondition['value'],
): SmartMailboxCondition {
  return { type: 'condition', field, operator, negated: false, value }
}

function renderCondition(
  condition: SmartMailboxCondition,
  data: ConditionEditorData = EMPTY_DATA,
) {
  const wrap = (node: ReactNode): ReactNode => (
    <QueryClientProvider client={new QueryClient()}>
      <ConditionEditorContext.Provider value={data}>
        {node}
      </ConditionEditorContext.Provider>
    </QueryClientProvider>
  )
  return render(
    wrap(
      <ConditionEditor
        condition={condition}
        onChange={() => {}}
        onRemove={() => {}}
      />,
    ),
  )
}

describe('ConditionEditor — type-directed value widget', () => {
  it('renders the date/relative picker (a native date input, not a text box) for a date field', () => {
    const { getByTestId, queryByTestId, container } = renderCondition(
      mkCondition('receivedAt', 'lt', ''),
    )

    expect(getByTestId('value-widget-date')).toBeDefined()
    expect(queryByTestId('value-widget-text')).toBeNull()
    // The marquee "auto-fill the format" case: a real date picker, not a box
    // the user hand-types an ISO timestamp into.
    expect(container.querySelector('input[type="date"]')).not.toBeNull()
  })

  it('renders the reused mailbox picker for mailboxId (not a text box)', () => {
    const mailboxes: Mailbox[] = [
      { id: 'mbx-1', name: 'Receipts', role: null } as Mailbox,
    ]
    const { getByTestId, queryByTestId, getByRole } = renderCondition(
      mkCondition('mailboxId', 'equals', ''),
      { accountId: 'acct', mailboxes, accounts: [] },
    )

    expect(getByTestId('value-widget-mailbox')).toBeDefined()
    // The reused move-action picker is a Select (combobox), never a text box.
    expect(getByRole('combobox', { name: 'Value' })).toBeDefined()
    expect(queryByTestId('value-widget-text')).toBeNull()
  })

  it('renders the account picker for sourceId', () => {
    const { getByTestId, queryByTestId } = renderCondition(
      mkCondition('sourceId', 'equals', ''),
      {
        accountId: '',
        mailboxes: null,
        accounts: [{ id: 'a1', name: 'Work' }],
      },
    )
    expect(getByTestId('value-widget-account')).toBeDefined()
    expect(queryByTestId('value-widget-text')).toBeNull()
  })

  it('renders the role select for mailboxRole', () => {
    const { getByTestId, queryByTestId } = renderCondition(
      mkCondition('mailboxRole', 'equals', ''),
    )
    expect(getByTestId('value-widget-role')).toBeDefined()
    expect(queryByTestId('value-widget-text')).toBeNull()
  })

  it('still renders the boolean Select for boolean fields', () => {
    const { getByTestId, getByRole, queryByTestId } = renderCondition(
      mkCondition('isRead', 'equals', false),
    )
    expect(getByTestId('value-widget-boolean')).toBeDefined()
    expect(getByRole('combobox', { name: 'Value' })).toBeDefined()
    expect(queryByTestId('value-widget-text')).toBeNull()
  })

  it('falls back to the text box for an un-mapped text field (no regression)', () => {
    const { getByTestId, getByRole } = renderCondition(
      mkCondition('subject', 'equals', ''),
    )
    expect(getByTestId('value-widget-text')).toBeDefined()
    expect(getByRole('textbox', { name: 'Value' })).toBeDefined()
  })

  it('renders the list editor with a text entry for the in operator', () => {
    const { getByTestId, getByRole, queryByTestId } = renderCondition(
      mkCondition('subject', 'in', []),
    )
    expect(getByTestId('value-widget-list')).toBeDefined()
    expect(getByRole('textbox', { name: 'Add value' })).toBeDefined()
    expect(queryByTestId('value-widget-text')).toBeNull()
  })

  it('renders existing in-list entries as removable chips', () => {
    const { getByTestId, getByRole } = renderCondition(
      mkCondition('subject', 'in', ['invoice', 'receipt']),
    )
    const list = getByTestId('value-widget-list')
    expect(list.textContent).toContain('invoice')
    expect(list.textContent).toContain('receipt')
    expect(getByRole('button', { name: 'Remove invoice' })).toBeDefined()
    expect(getByRole('button', { name: 'Remove receipt' })).toBeDefined()
  })

  it('composes the mailbox PICKER with the in operator (registry, not a bare box)', () => {
    // The old dispatch special-cased `in` into a comma-separated text box for
    // every type — the mailbox picker (and every other capability) vanished.
    // The registry composes valueType × arity, so `mailboxId in` gets the
    // picker as its list-entry adder.
    const mailboxes: Mailbox[] = [
      { id: 'mbx-1', name: 'Receipts', role: null } as Mailbox,
    ]
    const { getByTestId, getByRole } = renderCondition(
      mkCondition('mailboxId', 'in', []),
      { accountId: 'acct', mailboxes, accounts: [] },
    )
    expect(getByTestId('value-widget-list')).toBeDefined()
    expect(getByRole('combobox', { name: 'Add value' })).toBeDefined()
  })

  it('keeps the address AUTOCOMPLETE under the in operator ("is one of")', () => {
    // The owner-reported bug: switching an address field to "is one of"
    // dropped to a dumb comma list with no suggestions. The address entry is
    // now the same RecipientSuggestionInput the scalar path uses.
    const { getByTestId, getByRole, queryByTestId } = renderCondition(
      mkCondition('fromEmail', 'in', ['ada@example.com']),
    )
    expect(getByTestId('value-widget-list')).toBeDefined()
    // The adder is a labelled text input (the autocomplete component), and the
    // committed entry renders as a removable chip.
    expect(getByRole('textbox', { name: 'Add value' })).toBeDefined()
    expect(
      getByRole('button', { name: 'Remove ada@example.com' }),
    ).toBeDefined()
    expect(queryByTestId('value-widget-address')).toBeNull()
  })

  it('offers the keyword suggestion input for keyword fields (scalar + in)', () => {
    const scalar = renderCondition(mkCondition('keyword', 'equals', ''))
    expect(scalar.getByTestId('value-widget-keyword')).toBeDefined()
    expect(scalar.getByRole('textbox', { name: 'Value' })).toBeDefined()
    scalar.unmount()

    const list = renderCondition(mkCondition('keyword', 'in', []))
    expect(list.getByTestId('value-widget-list')).toBeDefined()
    expect(list.getByRole('textbox', { name: 'Add value' })).toBeDefined()
  })

  it('renders the number+unit widget for the size field (not a bare text box)', () => {
    const { getByTestId, getByRole, queryByTestId } = renderCondition(
      mkCondition('size', 'gt', ''),
    )
    expect(getByTestId('value-widget-size')).toBeDefined()
    // A numeric amount input plus a unit combobox — the "size + unit" case.
    expect(getByRole('spinbutton', { name: 'Value' })).toBeDefined()
    expect(getByRole('combobox', { name: 'Unit' })).toBeDefined()
    expect(queryByTestId('value-widget-text')).toBeNull()
  })

  it('renders the shared address autocomplete for the To recipient field', () => {
    // `to` is an address field: it now shares the compose recipient
    // autocomplete (the persistent address book), not a bare text box, while
    // still emitting the identical string wire shape.
    const { getByTestId, queryByTestId, getByRole } = renderCondition(
      mkCondition('to', 'contains', ''),
    )
    expect(getByTestId('value-widget-address')).toBeDefined()
    expect(queryByTestId('value-widget-text')).toBeNull()
    // The reused RecipientSuggestionInput is a labelled text input.
    expect(getByRole('textbox', { name: 'Value' })).toBeDefined()
  })

  it('renders the shared address autocomplete for the fromEmail field', () => {
    const { getByTestId, queryByTestId } = renderCondition(
      mkCondition('fromEmail', 'contains', ''),
    )
    expect(getByTestId('value-widget-address')).toBeDefined()
    expect(queryByTestId('value-widget-text')).toBeNull()
  })

  it('shows the existing stored value in the reused picker even with no account context', () => {
    // Smart-mailbox editor path: no account scope, but a previously-saved
    // mailbox id must still round-trip (interim: picker with the raw id shown).
    const { getByTestId } = renderCondition(
      mkCondition('mailboxId', 'equals', 'mbx-legacy'),
    )
    expect(getByTestId('value-widget-mailbox')).toBeDefined()
  })
})
