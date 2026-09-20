/**
 * The redaction gateway.
 *
 * Everything that could be sent to DeepSeek passes through here first. The
 * gateway replaces identifiers with stable pseudonyms, keeps amounts and dates
 * (which is what the answer actually depends on), and then RE-SCANS ITS OWN
 * OUTPUT. If anything still looks like personal data, it throws instead of
 * sending. A privacy feature that only usually works is not a privacy feature.
 */

import { classifyPtTaxNumber } from '../core/nif.ts';
import type { ChatMessage, PiiKind, RedactedPayload, Substitution } from './types.ts';

export interface RedactOptions {
  /** Client and supplier names known to the local vault. */
  knownParties?: readonly { name: string; kind: 'CLIENTE' | 'FORNECEDOR' }[];
  /** The taxpayer's own name, always redacted. */
  selfName?: string;
  /** Pseudonyms already assigned, so `CLIENTE_1` stays the same client over time. */
  existing?: ReadonlyMap<string, string>;
}

export interface RedactResult {
  text: string;
  substitutions: Substitution[];
  mapping: Map<string, string>;
}

export interface RedactPreview {
  readonly before: string;
  readonly after: string;
  readonly substitutions: Substitution[];
}

const IBAN_PATTERN = /\b[A-Z]{2}\d{2}(?:[ ]?[A-Z0-9]{4}){3,7}[ ]?[A-Z0-9]{1,4}\b/g;
const EMAIL_PATTERN = /\b[\w.+-]+@[\w-]+(?:\.[\w-]+)+\b/g;
const ATCUD_PATTERN = /\b[A-Z0-9]{8}-\d{1,6}\b/g;
const DOCUMENT_PATTERN = /\b(?:FR|FT|FS|NC|ND|RC|VD)\s?\d{4}\/\d{1,6}\b/gi;
/** 9-digit Portuguese numbers; the checksum decides whether it is a NIF or a phone. */
const NINE_DIGITS_PATTERN = /\b\d{9}\b/g;
const ADDRESS_PATTERN =
  /\b(?:Rua|Avenida|Av\.|Travessa|Praça|Largo|Estrada|Alameda|Beco|Calçada|Quinta|Urbanização)\s+[^\n,;]{3,60}/gi;

/**
 * Classify a nine-digit run.
 *
 * Privacy beats precision here: ANY nine-digit number is treated as an
 * identifier and redacted. When the checksum passes we can name it (NIF for a
 * natural person, NIPC for a company); when it fails it is still nine digits and
 * still not something we are willing to send, so it becomes IDENTIFICADOR.
 * Over-redacting costs the model nothing. Under-redacting costs the user their
 * privacy, and the user cannot audit a request they never saw.
 */
function classifyNineDigits(value: string): PiiKind {
  return classifyPtTaxNumber(value) ?? 'IDENTIFICADOR';
}

class TokenFactory {
  private readonly counters = new Map<PiiKind, number>();
  private readonly assigned = new Map<string, string>();

  constructor(existing?: ReadonlyMap<string, string>) {
    if (existing === undefined) return;
    for (const [original, token] of existing) {
      this.assigned.set(original, token);
      const match = /^([A-Z_]+)_(\d+)$/.exec(token);
      if (match?.[1] !== undefined && match[2] !== undefined) {
        const kind = match[1] as PiiKind;
        this.counters.set(kind, Math.max(this.counters.get(kind) ?? 0, Number(match[2])));
      }
    }
  }

  tokenFor(kind: PiiKind, original: string): string {
    const known = this.assigned.get(original);
    if (known !== undefined) return known;
    const next = (this.counters.get(kind) ?? 0) + 1;
    this.counters.set(kind, next);
    const token = `${kind}_${next}`;
    this.assigned.set(original, token);
    return token;
  }

  get mapping(): Map<string, string> {
    return new Map(this.assigned);
  }
}

function replacePattern(
  text: string,
  pattern: RegExp,
  resolve: (match: string) => { kind: PiiKind; original: string } | null,
  factory: TokenFactory,
  substitutions: Substitution[],
): string {
  return text.replace(pattern, (match) => {
    const resolved = resolve(match);
    if (resolved === null) return match;
    const token = factory.tokenFor(resolved.kind, resolved.original);
    if (!substitutions.some((entry) => entry.token === token)) {
      substitutions.push({ kind: resolved.kind, token, original: resolved.original });
    }
    return `«${token}»`;
  });
}

/**
 * Replace identifiable data with pseudonyms, preserving amounts, dates and the
 * structure of the text so the model can still reason about it.
 */
export function redactText(input: string, options: RedactOptions = {}): RedactResult {
  const factory = new TokenFactory(options.existing);
  const substitutions: Substitution[] = [];
  let text = input;

  // The taxpayer themselves, first, because their name may also appear in an address.
  if (options.selfName !== undefined && options.selfName.trim() !== '') {
    const self = options.selfName.trim();
    const escaped = self.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    text = replacePattern(
      text,
      new RegExp(escaped, 'gi'),
      () => ({ kind: 'CONTRIBUINTE' as PiiKind, original: self }),
      factory,
      substitutions,
    );
  }

  text = replacePattern(text, IBAN_PATTERN, (match) => ({ kind: 'IBAN', original: match }), factory, substitutions);
  text = replacePattern(text, EMAIL_PATTERN, (match) => ({ kind: 'EMAIL', original: match }), factory, substitutions);
  text = replacePattern(text, ATCUD_PATTERN, (match) => ({ kind: 'DOCUMENTO', original: match }), factory, substitutions);
  text = replacePattern(text, DOCUMENT_PATTERN, (match) => ({ kind: 'DOCUMENTO', original: match }), factory, substitutions);

  text = replacePattern(
    text,
    NINE_DIGITS_PATTERN,
    (match) => ({ kind: classifyNineDigits(match), original: match }),
    factory,
    substitutions,
  );

  text = replacePattern(text, ADDRESS_PATTERN, (match) => ({ kind: 'MORADA', original: match }), factory, substitutions);

  for (const party of options.knownParties ?? []) {
    const name = party.name.trim();
    if (name.length < 3) continue;
    const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    text = replacePattern(
      text,
      new RegExp(escaped, 'gi'),
      () => ({ kind: party.kind as PiiKind, original: name }),
      factory,
      substitutions,
    );
  }

  return { text, substitutions, mapping: factory.mapping };
}

/** What still looks like personal data after redaction, if anything. */
export function residualPii(text: string): PiiKind[] {
  const found = new Set<PiiKind>();
  for (const match of text.matchAll(NINE_DIGITS_PATTERN)) {
    found.add(classifyNineDigits(match[0]));
  }
  for (const match of text.matchAll(IBAN_PATTERN)) {
    if (match[0].length >= 15) found.add('IBAN');
  }
  if (text.match(EMAIL_PATTERN) !== null) found.add('EMAIL');
  return [...found];
}

export class RedactionError extends Error {
  readonly kinds: PiiKind[];

  constructor(kinds: PiiKind[]) {
    super(
      `A redação falhou: o texto ainda contém ${kinds.join(', ')}. Nada foi enviado. ` +
        'Corrige a expressão de deteção antes de tentar novamente.',
    );
    this.name = 'RedactionError';
    this.kinds = kinds;
  }
}

/**
 * Build the only payload the network client will accept.
 * Throws if the redaction did not actually remove everything it should have.
 */
export function redactForSend(messages: readonly ChatMessage[], options: RedactOptions = {}): RedactedPayload {
  const substitutions: Substitution[] = [];
  // Pseudonyms must be stable ACROSS messages too, or the model sees one client
  // introduced twice under two names and reasons about two different clients.
  const mapping = new Map<string, string>(options.existing ?? []);
  const redactedMessages: ChatMessage[] = messages.map((message) => {
    const result = redactText(message.content, { ...options, existing: mapping });
    for (const substitution of result.substitutions) {
      if (!substitutions.some((entry) => entry.token === substitution.token)) {
        substitutions.push(substitution);
      }
    }
    for (const [original, token] of result.mapping) mapping.set(original, token);
    return { role: message.role, content: result.text };
  });

  const residual = new Set<PiiKind>();
  for (const message of redactedMessages) {
    for (const kind of residualPii(message.content)) residual.add(kind);
  }
  if (residual.size > 0) throw new RedactionError([...residual]);

  const charactersSent = redactedMessages.reduce((total, message) => total + message.content.length, 0);
  return {
    messages: redactedMessages,
    substitutions,
    charactersSent,
  } as RedactedPayload;
}

/** Show the user exactly what would leave the machine, before it leaves. */
export function previewRedaction(before: string, options: RedactOptions = {}): RedactPreview {
  const result = redactText(before, options);
  return { before, after: result.text, substitutions: result.substitutions };
}

/** Put the real values back into the model's answer, locally, for display. */
export function restorePseudonyms(text: string, substitutions: readonly Substitution[]): string {
  let restored = text;
  for (const substitution of substitutions) {
    restored = restored.split(`«${substitution.token}»`).join(substitution.original);
    restored = restored.split(substitution.token).join(substitution.original);
  }
  return restored;
}
