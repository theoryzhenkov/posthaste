import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type SetStateAction,
} from 'react'

import type { Identity, ReplyContext } from '@/api/types'
import type { ComposeIntent } from '@/composeIntent'

import type { MarkdownComposerEditorHandle } from '../MarkdownComposerEditor'
import {
  EMPTY_FORM,
  composeAttachmentFromFile,
  formatRecipient,
  formatRecipients,
  type ComposeForm,
} from '../composeFormHelpers'
import { validateAttachmentLimits } from './attachments'

export function useComposeFormState({
  composeKey,
  identity,
  intentKind,
  isMessageBasedCompose,
  replyContext,
}: {
  composeKey: string
  identity: Identity | undefined
  intentKind: ComposeIntent['kind']
  isMessageBasedCompose: boolean
  replyContext: ReplyContext | undefined
}) {
  const bodyRef = useRef<MarkdownComposerEditorHandle>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const initialForm = useMemo<ComposeForm>(() => {
    if (intentKind === 'new' || !replyContext) {
      return EMPTY_FORM
    }
    const quoted = replyContext.quotedBody
      ? `\n\n${replyContext.quotedBody}`
      : ''
    return {
      from: '',
      to: intentKind === 'reply' ? formatRecipients(replyContext.to) : '',
      cc: '',
      bcc: '',
      subject:
        intentKind === 'reply'
          ? replyContext.replySubject
          : replyContext.forwardSubject,
      body: quoted,
      attachments: [],
    }
  }, [intentKind, replyContext])
  const formResetKey = isMessageBasedCompose
    ? `${composeKey}:${replyContext ? 'ready' : 'loading'}`
    : composeKey
  const [composeState, setComposeState] = useState(() => ({
    errorMessage: null as string | null,
    form: initialForm,
    resetKey: formResetKey,
  }))
  const [fromMenuOpen, setFromMenuOpen] = useState(false)
  const [fromInputFocused, setFromInputFocused] = useState(false)
  const [editedResetKey, setEditedResetKey] = useState<string | null>(null)
  const [isReadingAttachments, setIsReadingAttachments] = useState(false)
  const editedResetKeyRef = useRef<string | null>(null)

  const needsFormReset = composeState.resetKey !== formResetKey
  const form = needsFormReset ? initialForm : composeState.form
  const errorMessage = needsFormReset ? null : composeState.errorMessage
  const setForm = useCallback(
    (nextForm: SetStateAction<ComposeForm>) => {
      setComposeState((current) => {
        const isCurrentForm = current.resetKey === formResetKey
        const baseForm = isCurrentForm ? current.form : initialForm
        return {
          errorMessage: isCurrentForm ? current.errorMessage : null,
          form: typeof nextForm === 'function' ? nextForm(baseForm) : nextForm,
          resetKey: formResetKey,
        }
      })
    },
    [formResetKey, initialForm],
  )
  const setErrorMessage = useCallback(
    (message: string | null) => {
      setComposeState((current) => {
        const isCurrentForm = current.resetKey === formResetKey
        return {
          errorMessage: message,
          form: isCurrentForm ? current.form : initialForm,
          resetKey: formResetKey,
        }
      })
    },
    [formResetKey, initialForm],
  )
  const setField = useCallback(
    <K extends keyof ComposeForm>(field: K, value: ComposeForm[K]) => {
      editedResetKeyRef.current = formResetKey
      setEditedResetKey(formResetKey)
      setForm((current) => ({ ...current, [field]: value }))
    },
    [formResetKey, setForm],
  )
  const handleBodyChange = useCallback(
    (value: string) => setField('body', value),
    [setField],
  )
  const removeAttachment = useCallback(
    (attachmentId: string) => {
      setField(
        'attachments',
        form.attachments.filter((attachment) => attachment.id !== attachmentId),
      )
    },
    [form.attachments, setField],
  )
  const handleAttachFiles = useCallback(
    (files: FileList | null) => {
      if (!files || files.length === 0) {
        return
      }
      const nextAttachments = [
        ...form.attachments,
        ...Array.from(files).map(composeAttachmentFromFile),
      ]
      const error = validateAttachmentLimits(nextAttachments)
      if (error) {
        setErrorMessage(error)
      } else {
        setErrorMessage(null)
        setField('attachments', nextAttachments)
      }
      if (fileInputRef.current) {
        fileInputRef.current.value = ''
      }
    },
    [form.attachments, setErrorMessage, setField],
  )

  useEffect(() => {
    if (isMessageBasedCompose && replyContext) {
      requestAnimationFrame(() => bodyRef.current?.focus())
    }
  }, [composeKey, isMessageBasedCompose, replyContext])

  useEffect(() => {
    if (!identity || form.from.trim().length > 0) {
      return
    }
    const frame = requestAnimationFrame(() => {
      setForm((current) =>
        current.from.trim().length > 0
          ? current
          : { ...current, from: formatRecipient(identity) },
      )
    })

    return () => cancelAnimationFrame(frame)
  }, [form.from, identity, setForm])

  return {
    bodyRef,
    editedResetKeyRef,
    errorMessage,
    fileInputRef,
    form,
    formResetKey,
    fromInputFocused,
    fromMenuOpen,
    handleAttachFiles,
    handleBodyChange,
    hasUserEdited: editedResetKey === formResetKey,
    isReadingAttachments,
    removeAttachment,
    setErrorMessage,
    setField,
    setFromInputFocused,
    setFromMenuOpen,
    setIsReadingAttachments,
  }
}
