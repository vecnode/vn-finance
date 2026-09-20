/**
 * Types for the AI layer.
 *
 * `RedactedPayload` is branded on purpose. The DeepSeek client accepts nothing
 * else, so it is a TYPE ERROR to send raw financial text to the model: the only
 * way to obtain a payload is to pass text through `redactForSend`, which
 * substitutes identifiers and then re-scans its own output for anything that
 * still looks like personal data. The privacy guarantee is enforced by the
 * compiler, not by a code review.
 */

export type ChatRole = 'system' | 'user' | 'assistant';

export interface ChatMessage {
  role: ChatRole;
  content: string;
}

export type PiiKind =
  | 'CONTRIBUINTE'
  | 'NIF'
  | 'NIPC'
  | 'IBAN'
  | 'EMAIL'
  | 'TELEFONE'
  | 'MORADA'
  | 'CLIENTE'
  | 'FORNECEDOR'
  | 'DOCUMENTO'
  /** Nine digits whose checksum does not validate: still not sent. */
  | 'IDENTIFICADOR';

export interface Substitution {
  kind: PiiKind;
  /** The pseudonym that replaced the original, e.g. `NIF_1`. */
  token: string;
  /** Kept locally for the preview so the user can see exactly what is hidden. */
  original: string;
}

declare const redactedBrand: unique symbol;

/** Proof that a payload has passed through the redaction gateway. */
export interface RedactedPayload {
  readonly [redactedBrand]: true;
  readonly messages: ChatMessage[];
  readonly substitutions: Substitution[];
  /** Size of the payload actually sent, for the cost meter. */
  readonly charactersSent: number;
}
