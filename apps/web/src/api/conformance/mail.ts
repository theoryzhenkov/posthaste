import type {
  ConversationPage,
  ConversationSummary,
  ConversationView,
  DomainEvent,
  Mailbox,
  MessageAttachment,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  MessageSummary,
  PatchMailboxInput,
  RawMessageRef,
  SourceMessageRef,
  TagSummary,
} from '../types'
import type { AssertTrue, Conforms, Wire } from './core'

export type _Mailbox = AssertTrue<Conforms<Mailbox, Wire['MailboxSummary']>>
export type _PatchMailboxInput = AssertTrue<
  Conforms<PatchMailboxInput, Wire['PatchMailboxRequest']>
>
export type _MessageSummary = AssertTrue<
  Conforms<MessageSummary, Wire['MessageSummary']>
>
export type _MessagePage = AssertTrue<
  Conforms<MessagePage, Wire['MessagePageResponse']>
>
export type _RawMessageRef = AssertTrue<
  Conforms<RawMessageRef, Wire['RawMessageRef']>
>
export type _MessageAttachment = AssertTrue<
  Conforms<MessageAttachment, Wire['MessageAttachment']>
>
export type _MessageDetail = AssertTrue<
  Conforms<MessageDetail, Wire['MessageDetail']>
>
export type _SourceMessageRef = AssertTrue<
  Conforms<SourceMessageRef, Wire['SourceMessageRef']>
>
export type _MessageCommandResult = AssertTrue<
  Conforms<MessageCommandResult, Wire['CommandResult']>
>
export type _ConversationSummary = AssertTrue<
  Conforms<ConversationSummary, Wire['ConversationSummary']>
>
export type _ConversationPage = AssertTrue<
  Conforms<ConversationPage, Wire['ConversationPageResponse']>
>
export type _ConversationView = AssertTrue<
  Conforms<ConversationView, Wire['ConversationView']>
>
export type _TagSummary = AssertTrue<Conforms<TagSummary, Wire['TagSummary']>>
export type _DomainEvent = AssertTrue<
  Conforms<DomainEvent, Wire['DomainEvent']>
>
