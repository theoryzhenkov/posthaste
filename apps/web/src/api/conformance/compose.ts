import type {
  Identity,
  OkResponse,
  Recipient,
  ReplyContext,
  SendMessageInput,
} from '../types'
import type { AssertTrue, Conforms, Wire } from './core'

export type _Identity = AssertTrue<Conforms<Identity, Wire['Identity']>>
export type _Recipient = AssertTrue<Conforms<Recipient, Wire['Recipient']>>
export type _ReplyContext = AssertTrue<
  Conforms<ReplyContext, Wire['ReplyContext']>
>
export type _SendMessageInput = AssertTrue<
  Conforms<SendMessageInput, Wire['SendMessageRequest']>
>
export type _OkResponse = AssertTrue<Conforms<OkResponse, Wire['OkResponse']>>
