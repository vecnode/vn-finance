/**
 * The DeepSeek client — THE ONLY PLACE IN THIS CODEBASE THAT TOUCHES THE NETWORK.
 *
 * That is a deliberate, auditable property: `grep -rn "fetch(" src/` must return
 * exactly one hit, here. Everything else — the calendar, the tax arithmetic, the
 * invoice ledger, the document vault — works with the network cable unplugged.
 *
 * The method signature is the second half of the guarantee: `complete` accepts
 * only a `RedactedPayload`, which can only be produced by `redactForSend`. Raw
 * text cannot be sent even by accident.
 */

import type { RedactedPayload } from './types.ts';

export const DEEPSEEK_BASE_URL = 'https://api.deepseek.com';
export const DEFAULT_MODEL = 'deepseek-chat';

export interface DeepSeekRequestOptions {
  model?: string;
  temperature?: number;
  maxTokens?: number;
  timeoutMs?: number;
}

/** Prices in euro cents per million tokens. Filled from local configuration only. */
export interface PriceRow {
  inputCentsPerMillion: number;
  outputCentsPerMillion: number;
}

export type PriceTable = Readonly<Record<string, PriceRow>>;

export interface DeepSeekUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface DeepSeekAnswer {
  text: string;
  model: string;
  usage: DeepSeekUsage;
  /** Null when no local price table is configured: we do not guess prices. */
  costCents: number | null;
  requestCharacters: number;
}

export class DeepSeekError extends Error {
  readonly status: number | undefined;

  constructor(message: string, status?: number) {
    super(message);
    this.name = 'DeepSeekError';
    this.status = status;
  }
}

/**
 * The provider's error body can echo the credential back
 * (`"your api key: ****abcd is invalid"`). This application promises the key is
 * never logged, so anything credential-shaped is scrubbed before the message can
 * reach a terminal or the audit log.
 */
export function scrubCredentials(text: string): string {
  return text
    .replace(/sk-[A-Za-z0-9_-]{6,}/g, 'sk-…[redigido]')
    .replace(/\*{2,}[A-Za-z0-9]{2,}/g, '…[redigido]');
}

interface ChatCompletionResponse {
  model?: string;
  choices?: Array<{ message?: { content?: string } }>;
  usage?: { prompt_tokens?: number; completion_tokens?: number; total_tokens?: number };
}

export class DeepSeekClient {
  readonly #apiKey: string;
  readonly #baseUrl: string;
  readonly #prices: PriceTable;

  constructor(apiKey: string, options: { baseUrl?: string; prices?: PriceTable } = {}) {
    if (apiKey.trim() === '') throw new DeepSeekError('chave da API ausente');
    this.#apiKey = apiKey;
    this.#baseUrl = (options.baseUrl ?? DEEPSEEK_BASE_URL).replace(/\/$/, '');
    this.#prices = options.prices ?? {};
  }

  get hasPricing(): boolean {
    return Object.keys(this.#prices).length > 0;
  }

  /**
   * Send an already-redacted conversation. Nothing else can be passed here.
   */
  async complete(
    payload: RedactedPayload,
    options: DeepSeekRequestOptions = {},
  ): Promise<DeepSeekAnswer> {
    const model = options.model ?? DEFAULT_MODEL;
    const body = {
      model,
      messages: payload.messages,
      temperature: options.temperature ?? 0.2,
      max_tokens: options.maxTokens ?? 1200,
      stream: false,
    };

    let response: Response;
    try {
      response = await fetch(`${this.#baseUrl}/chat/completions`, {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          authorization: `Bearer ${this.#apiKey}`,
        },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(options.timeoutMs ?? 60_000),
      });
    } catch (cause) {
      throw new DeepSeekError(`falha de rede ao contactar a DeepSeek: ${(cause as Error).message}`);
    }

    if (!response.ok) {
      const detail = scrubCredentials((await response.text().catch(() => '')).slice(0, 400));
      throw new DeepSeekError(
        `a DeepSeek respondeu ${response.status}${detail === '' ? '' : `: ${detail}`}`,
        response.status,
      );
    }

    const parsed = (await response.json()) as ChatCompletionResponse;
    const text = parsed.choices?.[0]?.message?.content ?? '';
    const usage: DeepSeekUsage = {
      promptTokens: parsed.usage?.prompt_tokens ?? 0,
      completionTokens: parsed.usage?.completion_tokens ?? 0,
      totalTokens: parsed.usage?.total_tokens ?? 0,
    };

    return {
      text,
      model: parsed.model ?? model,
      usage,
      costCents: this.#estimateCost(parsed.model ?? model, usage),
      requestCharacters: payload.charactersSent,
    };
  }

  #estimateCost(model: string, usage: DeepSeekUsage): number | null {
    const row = this.#prices[model];
    if (row === undefined) return null;
    const input = (usage.promptTokens / 1_000_000) * row.inputCentsPerMillion;
    const output = (usage.completionTokens / 1_000_000) * row.outputCentsPerMillion;
    return Math.round(input + output);
  }
}
