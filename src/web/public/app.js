/* ==========================================================================
   vn-finance — painel local (front end).

   Três compromissos visíveis neste ficheiro:

     - NENHUMA REGRA FISCAL VIVE AQUI. Todos os números, prazos e percentagens
       vêm do modelo montado em `src/web/report.ts` pelo motor determinístico.
       Este ficheiro formata e dispõe; não decide.
     - AS ÚNICAS CONVERSÕES SÃO AS DO FORMULÁRIO: euros escritos por uma pessoa
       -> cêntimos, e percentagens escritas por uma pessoa -> pontos base. São
       o contrato da API (`baseCents`, `ivaRateBp`) a exigir a conversão, e é
       por isso que ela está isolada em `parseEurosToCents` / `parsePercentToBp`.
     - NADA É CARREGADO DE FORA. Sem CDN, sem fontes web, sem imagens, sem
       framework. A única chamada de rede é a que o utilizador autoriza em
       "Atualizar regras", e vai para o servidor local.
   ========================================================================== */

const TOKEN_KEY = 'vnfin.token';

const API = {
  dashboard: '/api/dashboard',
  profile: '/api/profile',
  profileImport: '/api/profile/import',
  profileExport: '/api/profile/export',
  invoices: '/api/invoices',
  complete: '/api/obligations/complete',
  documents: '/api/documents',
  updateSend: '/api/update/send',
  updateApply: '/api/update/apply',
  updateDiscard: '/api/update/discard',
};

/* --------------------------------------------------------------------------
   Estado do painel
   -------------------------------------------------------------------------- */

const state = {
  token: '',
  model: null,
  filter: 'year',
  preview: false,
  nifRevealed: false,
  ledger: { pt: null, foreign: null },
  /* O formulário de perfil abre sozinho na primeira utilização — quando o cofre
     ainda não tem perfil — e fica disponível no botão "Perfil" a partir daí. */
  profileOpen: false,
  profilePrompted: false,
};

/* --------------------------------------------------------------------------
   Vocabulário fixo (etiquetas de valores do contrato — não é lógica fiscal)
   -------------------------------------------------------------------------- */

const KIND_LABEL = {
  declare: 'Declarar',
  pay: 'Pagar',
  communicate: 'Comunicar',
  save: 'Conservar',
  review: 'Rever',
  submit: 'Submeter',
};

const TREATMENT_LABEL = {
  iva_pt: 'IVA português',
  autoliquidacao_ue: 'IVA — autoliquidação',
  isento_art53: 'isento pelo art. 53.º',
  exportacao: 'exportação',
};

const INVOICE_STATUS = {
  paid: { cls: 'p-ok', glyph: '✓', label: 'Pago' },
  pending: { cls: 'p-warn', glyph: '●', label: 'Pendente' },
  issued: { cls: 'p-future', glyph: '○', label: 'Emitido' },
};

const VERIFICATION = {
  verified: { cls: 'badge ver', glyph: '✓', label: 'verificada' },
  partial: { cls: 'badge est', glyph: '◐', label: 'parcial' },
  unverified: { cls: 'badge miss', glyph: '?', label: 'por verificar' },
  stale: { cls: 'badge miss', glyph: '▲', label: 'desatualizada' },
};

const FILTERS = [
  { id: '30', label: 'Próximos 30 dias' },
  { id: '90', label: 'Próximos 90 dias' },
  { id: 'year', label: 'Todo o ano' },
  { id: 'done', label: 'Concluídos' },
  { id: 'na', label: 'Não aplicáveis' },
];

/* Clientes: o grupo de cada país existe para pré-selecionar o tratamento de IVA
   e a retenção no formulário. É uma pré-seleção visível e editável, não um
   cálculo: o valor gravado é sempre o que estiver no formulário. */
const COUNTRIES = [
  { code: 'PT', name: 'Portugal', group: 'pt' },
  { code: 'ES', name: 'Espanha', group: 'ue' },
  { code: 'FR', name: 'França', group: 'ue' },
  { code: 'DE', name: 'Alemanha', group: 'ue' },
  { code: 'IT', name: 'Itália', group: 'ue' },
  { code: 'NL', name: 'Países Baixos', group: 'ue' },
  { code: 'BE', name: 'Bélgica', group: 'ue' },
  { code: 'LU', name: 'Luxemburgo', group: 'ue' },
  { code: 'IE', name: 'Irlanda', group: 'ue' },
  { code: 'AT', name: 'Áustria', group: 'ue' },
  { code: 'DK', name: 'Dinamarca', group: 'ue' },
  { code: 'SE', name: 'Suécia', group: 'ue' },
  { code: 'FI', name: 'Finlândia', group: 'ue' },
  { code: 'PL', name: 'Polónia', group: 'ue' },
  { code: 'CZ', name: 'Chéquia', group: 'ue' },
  { code: 'SK', name: 'Eslováquia', group: 'ue' },
  { code: 'HU', name: 'Hungria', group: 'ue' },
  { code: 'RO', name: 'Roménia', group: 'ue' },
  { code: 'BG', name: 'Bulgária', group: 'ue' },
  { code: 'HR', name: 'Croácia', group: 'ue' },
  { code: 'SI', name: 'Eslovénia', group: 'ue' },
  { code: 'EE', name: 'Estónia', group: 'ue' },
  { code: 'LV', name: 'Letónia', group: 'ue' },
  { code: 'LT', name: 'Lituânia', group: 'ue' },
  { code: 'CY', name: 'Chipre', group: 'ue' },
  { code: 'MT', name: 'Malta', group: 'ue' },
  { code: 'GR', name: 'Grécia', group: 'ue' },
  { code: 'GB', name: 'Reino Unido', group: 'terceiros' },
  { code: 'CH', name: 'Suíça', group: 'terceiros' },
  { code: 'NO', name: 'Noruega', group: 'terceiros' },
  { code: 'US', name: 'Estados Unidos', group: 'terceiros' },
  { code: 'CA', name: 'Canadá', group: 'terceiros' },
  { code: 'BR', name: 'Brasil', group: 'terceiros' },
  { code: 'AO', name: 'Angola', group: 'terceiros' },
  { code: 'MZ', name: 'Moçambique', group: 'terceiros' },
  { code: 'CV', name: 'Cabo Verde', group: 'terceiros' },
  { code: '__outro__', name: 'Outro país', group: 'terceiros' },
];

/* --------------------------------------------------------------------------
   Formatação. Nada aqui calcula imposto: converte unidades para leitura.
   -------------------------------------------------------------------------- */

const EUR = new Intl.NumberFormat('pt-PT', {
  style: 'currency',
  currency: 'EUR',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});
const DEC = new Intl.NumberFormat('pt-PT', { maximumFractionDigits: 2 });
const DEC4 = new Intl.NumberFormat('pt-PT', { maximumFractionDigits: 4 });
const INT = new Intl.NumberFormat('pt-PT');
const MONTH_FMT = new Intl.DateTimeFormat('pt-PT', { month: 'long', year: 'numeric', timeZone: 'UTC' });
const WEEKDAY_FMT = new Intl.DateTimeFormat('pt-PT', { weekday: 'long', timeZone: 'UTC' });

function esc(value) {
  return String(value === null || value === undefined ? '' : value).replace(
    /[&<>"']/g,
    (character) =>
      ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character],
  );
}

/** Escape that keeps line breaks, for flag details that list several deadlines. */
function escLines(value) {
  return esc(value).replace(/\r?\n/g, '<br>');
}

function safeUrl(value) {
  return typeof value === 'string' && /^https?:\/\//i.test(value) ? esc(value) : null;
}

function eur(cents) {
  if (typeof cents !== 'number' || !Number.isFinite(cents)) return '—';
  return EUR.format(cents / 100).replace(/\u00a0/g, '\u202f');
}

function bpPercent(bp) {
  if (typeof bp !== 'number' || !Number.isFinite(bp)) return '—';
  return `${DEC.format(bp / 100)}%`;
}

function bpCoefficient(bp) {
  if (typeof bp !== 'number' || !Number.isFinite(bp)) return '—';
  return DEC4.format(bp / 10000);
}

function isIsoDate(value) {
  return typeof value === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(value);
}

function ptDate(iso) {
  if (!isIsoDate(iso)) return '—';
  return `${iso.slice(8, 10)}/${iso.slice(5, 7)}/${iso.slice(0, 4)}`;
}

/** ISO timestamp or date -> "dd/mm/yyyy" or "dd/mm/yyyy hh:mm". */
function stamp(value) {
  if (typeof value !== 'string' || value.length < 10) return '—';
  const date = ptDate(value.slice(0, 10));
  const time = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(value) ? ` ${value.slice(11, 16)}` : '';
  return date + time;
}

function monthLabel(iso) {
  if (!isIsoDate(iso)) return '—';
  const date = new Date(Date.UTC(Number(iso.slice(0, 4)), Number(iso.slice(5, 7)) - 1, 1));
  const text = MONTH_FMT.format(date);
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function weekdayLabel(iso) {
  if (!isIsoDate(iso)) return '';
  const date = new Date(Date.UTC(Number(iso.slice(0, 4)), Number(iso.slice(5, 7)) - 1, Number(iso.slice(8, 10))));
  return WEEKDAY_FMT.format(date);
}

function daysBetween(from, to) {
  if (!isIsoDate(from) || !isIsoDate(to)) return Number.NaN;
  const millis = (value) => Date.UTC(Number(value.slice(0, 4)), Number(value.slice(5, 7)) - 1, Number(value.slice(8, 10)));
  return Math.round((millis(to) - millis(from)) / 86400000);
}

function bytesLabel(bytes) {
  if (typeof bytes !== 'number' || !Number.isFinite(bytes)) return '—';
  if (bytes < 1024) return `${INT.format(bytes)} B`;
  if (bytes < 1024 * 1024) return `${DEC.format(bytes / 1024)} KB`;
  return `${DEC.format(bytes / (1024 * 1024))} MB`;
}

/** 123456789 -> 123••••89 (primeiros três e últimos dois dígitos). */
function maskNif(nif) {
  const digits = String(nif || '').replace(/[\s.\-]/g, '');
  if (digits.length <= 4) return '••••';
  const head = digits.slice(0, Math.min(3, digits.length - 2));
  const tail = digits.slice(-2);
  return head + '•'.repeat(Math.max(0, digits.length - head.length - tail.length)) + tail;
}

/**
 * Euros escritos por uma pessoa -> cêntimos inteiros. Convenção portuguesa:
 * a vírgula é o separador decimal, o ponto e o espaço são de milhares.
 * Devolve `null` para vazio e `undefined` para algo que não é um valor.
 */
function parseEurosToCents(input) {
  const cleaned = String(input === null || input === undefined ? '' : input)
    .replace(/[€\s\u00a0\u202f]/g, '')
    .trim();
  if (cleaned === '') return null;
  const lastComma = cleaned.lastIndexOf(',');
  const lastDot = cleaned.lastIndexOf('.');
  let normalised;
  if (lastComma >= 0 && lastDot >= 0) {
    normalised = lastComma > lastDot ? cleaned.replace(/\./g, '').replace(',', '.') : cleaned.replace(/,/g, '');
  } else if (lastComma >= 0) {
    normalised = cleaned.replace(',', '.');
  } else {
    normalised = cleaned;
  }
  if (!/^-?\d+(\.\d{1,2})?$/.test(normalised)) return undefined;
  return Math.round(Number(normalised) * 100);
}

/** Percentagem escrita por uma pessoa ("21,4") -> pontos base (2140). */
function parsePercentToBp(input) {
  const cleaned = String(input === null || input === undefined ? '' : input)
    .replace(/[%\s\u00a0\u202f]/g, '')
    .replace(',', '.');
  if (cleaned === '') return null;
  if (!/^-?\d+(\.\d{1,4})?$/.test(cleaned)) return undefined;
  return Math.round(Number(cleaned) * 100);
}

function bpToInput(bp) {
  if (typeof bp !== 'number' || !Number.isFinite(bp)) return '';
  return String(bp / 100).replace('.', ',');
}

function centsToInput(cents) {
  if (typeof cents !== 'number' || !Number.isFinite(cents)) return '';
  return (cents / 100).toFixed(2).replace('.', ',');
}

/**
 * Euros por extenso -> cêntimos, para os valores do perfil.
 *
 * Ao contrário de `parseEurosToCents`, um campo vazio significa "apagar o valor
 * guardado" (`null`), porque é isso que o formulário do perfil oferece: a mesma
 * caixa serve para escrever e para remover um número que estava errado.
 */
function parseCentsField(input) {
  const cleaned = String(input === null || input === undefined ? '' : input)
    .replace(/[€\s\u00a0\u202f]/g, '')
    .trim();
  if (cleaned === '') return null;
  const cents = parseEurosToCents(cleaned);
  if (cents === undefined || cents === null || cents < 0) return undefined;
  return cents;
}

/* --------------------------------------------------------------------------
   Chave de sessão: lida do URL, guardada na sessão, retirada da barra
   -------------------------------------------------------------------------- */

function bootstrapToken() {
  let fromUrl = null;
  try {
    const params = new URLSearchParams(window.location.search);
    fromUrl = params.get('t');
    if (fromUrl !== null) {
      params.delete('t');
      const query = params.toString();
      const clean = window.location.pathname + (query === '' ? '' : `?${query}`) + window.location.hash;
      window.history.replaceState(null, '', clean);
    }
  } catch {
    fromUrl = null;
  }

  if (fromUrl !== null && fromUrl.trim() !== '') {
    try {
      window.sessionStorage.setItem(TOKEN_KEY, fromUrl);
    } catch {
      /* A sessão pode estar indisponível: a chave continua a valer nesta página. */
    }
    return fromUrl;
  }
  try {
    return window.sessionStorage.getItem(TOKEN_KEY) || '';
  } catch {
    return '';
  }
}

/* --------------------------------------------------------------------------
   Cliente da API local
   -------------------------------------------------------------------------- */

class ApiError extends Error {}

async function request(path, method = 'GET', body) {
  const headers = { 'X-VNFIN-Token': state.token, Accept: 'application/json' };
  const init = { method, headers, credentials: 'omit', cache: 'no-store' };
  if (body !== undefined) {
    headers['Content-Type'] = 'application/json';
    init.body = JSON.stringify(body);
  }

  let response;
  try {
    response = await fetch(path, init);
  } catch {
    throw new ApiError(
      `Não foi possível contactar o servidor local (${path}). Confirma que o vnfin web ainda está a correr.`,
    );
  }

  const text = await response.text();
  let data = null;
  if (text !== '') {
    try {
      data = JSON.parse(text);
    } catch {
      data = null;
    }
  }

  if (!response.ok || data === null || data.ok === false) {
    const message =
      data !== null && typeof data.error === 'string' && data.error !== ''
        ? data.error
        : `O servidor local respondeu ${response.status}${response.statusText ? ` ${response.statusText}` : ''}.`;
    throw new ApiError(message);
  }
  return data;
}

/** Um POST devolve sempre o modelo completo: re-renderizar, nunca remendar. */
async function send(path, body) {
  const data = await request(path, 'POST', body);
  const model = data !== null && typeof data === 'object' ? data.model : null;
  if (model === null || typeof model !== 'object') {
    throw new ApiError('A resposta do servidor não incluiu o modelo atualizado.');
  }
  applyModel(model);
  return data;
}

function applyModel(model) {
  state.model = model;
  computeLedgerDefaults(model.invoices ?? []);
  const scroll = window.scrollY;
  render(model);
  window.scrollTo(0, scroll);
}

/* --------------------------------------------------------------------------
   Avisos: banner de erro (com repetição) e confirmação breve
   -------------------------------------------------------------------------- */

let toastTimer = null;

function toast(message, kind = 'ok') {
  const node = document.getElementById('toast');
  if (node === null) return;
  node.className = `toast ${kind === 'err' ? 'err' : 'ok'}`;
  node.innerHTML = `<span class="g" aria-hidden="true">${kind === 'err' ? '▲' : '✓'}</span> ${esc(message)}`;
  node.hidden = false;
  window.requestAnimationFrame(() => node.classList.add('on'));
  if (toastTimer !== null) window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    node.classList.remove('on');
    toastTimer = window.setTimeout(() => {
      node.hidden = true;
    }, 200);
  }, 5200);
}

function showError(message) {
  const host = document.getElementById('aviso');
  if (host === null) return;
  host.innerHTML =
    '<div class="banner banner-err">' +
    '<span class="sev sev-alta"><span class="g" aria-hidden="true">▲</span> Erro</span>' +
    `<div><p class="b-t">O servidor local recusou o pedido</p><p class="b-d">${esc(message)}</p></div>` +
    '<div class="b-a"><button type="button" class="btn btn-sm" data-act="retry">Tentar de novo</button>' +
    '<button type="button" class="btn btn-sm btn-tertiary" data-act="dismiss-error">Fechar</button></div>' +
    '</div>';
  host.scrollIntoView({ block: 'nearest' });
}

function clearError() {
  const host = document.getElementById('aviso');
  if (host !== null) host.innerHTML = '';
}

/* --------------------------------------------------------------------------
   Peças pequenas reutilizadas
   -------------------------------------------------------------------------- */

function documentedRules(model) {
  const ids = new Set();
  for (const entry of model.vault?.entries ?? []) {
    if (typeof entry.obligationId === 'string' && entry.obligationId !== '') ids.add(entry.obligationId);
  }
  return ids;
}

function needsDocumentation(instance) {
  return (instance.documentsToKeep?.length ?? 0) > 0;
}

function isActionable(instance) {
  return instance.status !== 'done' && instance.status !== 'not_applicable' && instance.status !== 'untracked';
}

function statusPill(instance, today) {
  switch (instance.status) {
    case 'done':
      return { cls: 'p-ok', glyph: '✓', label: 'Concluído' };
    case 'not_applicable':
      return { cls: 'p-na', glyph: '⊘', label: 'Não aplicável' };
    case 'untracked':
      return { cls: 'p-future', glyph: '—', label: 'Histórico' };
    case 'overdue':
      return { cls: 'p-danger', glyph: '▲', label: 'Em atraso' };
    case 'due_soon': {
      const delta = daysBetween(today, instance.dueDate);
      if (delta === 0) return { cls: 'p-danger', glyph: '▲', label: 'Vence hoje' };
      if (delta === 1) return { cls: 'p-danger', glyph: '▲', label: 'Vence amanhã' };
      return { cls: 'p-warn', glyph: '●', label: 'Pendente' };
    }
    default:
      return { cls: 'p-future', glyph: '○', label: 'Futuro' };
  }
}

function fonteBadge(instance) {
  if (instance.provisional === true) return { cls: 'badge est', text: 'provisória' };
  if (instance.dueDateSource === 'official') {
    return { cls: 'badge', text: instance.adjustedForWeekend === true ? 'oficial · dia útil' : 'oficial' };
  }
  return { cls: 'badge est', text: 'calculada' };
}

function sectionHead(title, hint) {
  return (
    '<div class="block-head">' +
    `<h2>${esc(title)}</h2>` +
    (hint ? `<span class="hint">${hint}</span>` : '') +
    '</div>'
  );
}

function emptyState(title, text) {
  return `<div class="empty"><strong>${esc(title)}</strong>${esc(text)}</div>`;
}

function notes(strings, kind = 'warn') {
  const list = (strings ?? []).filter((item) => typeof item === 'string' && item.trim() !== '');
  if (list.length === 0) return '';
  return (
    '<div class="notes">' +
    list
      .map(
        (item) =>
          `<p><span class="g${kind === 'danger' ? ' danger' : ''}" aria-hidden="true">!</span><span>${esc(item)}</span></p>`,
      )
      .join('') +
    '</div>'
  );
}

function tableWrap(caption, head, body) {
  return (
    '<div class="tbl"><table>' +
    `<caption class="sr-only">${esc(caption)}</caption>` +
    `<thead><tr>${head}</tr></thead>` +
    `<tbody>${body}</tbody>` +
    '</table></div>'
  );
}

/* --------------------------------------------------------------------------
   Barra lateral, barra superior e rodapé de impressão
   -------------------------------------------------------------------------- */

function setText(id, text) {
  const node = document.getElementById(id);
  if (node !== null) node.textContent = text;
}

function setCount(id, text, variant = '') {
  const node = document.getElementById(id);
  if (node === null) return;
  node.className = variant === '' ? 'cnt' : `cnt ${variant}`;
  node.textContent = text;
}

function ivaRegimeLabel(regime) {
  switch (regime) {
    case 'isento_art53':
      return 'isento (art. 53.º)';
    case 'mensal':
      return 'mensal';
    case 'trimestral':
      return 'trimestral';
    default:
      return regime ? String(regime) : '—';
  }
}

function irsRegimeLabel(regime) {
  switch (regime) {
    case 'simplificado':
      return 'simplificado';
    case 'organizada':
      return 'contabilidade organizada';
    default:
      return regime ? String(regime) : '—';
  }
}

function renderChrome(model) {
  const meta = model.meta;
  const profile = model.profile;
  const summary = model.pack?.summary ?? {};
  const counts = model.flagSummary ?? { urgent: 0, attention: 0, info: 0, total: 0 };

  document.title = `vn-finance · Painel ${meta.year}`;
  setText('tb-sub', `Exercício ${meta.year} · trabalhador independente · CIRS categoria B`);
  setText('chip-ano', String(meta.year));
  setText('chip-iva', profile === null ? 'IVA —' : `IVA ${ivaRegimeLabel(profile.iva?.regime)}`);
  setText('chip-irs', profile === null ? 'IRS —' : `IRS ${irsRegimeLabel(profile.irs?.regime)}`);
  setText('today-tag', `${ptDate(meta.today)} · hoje`);
  setText('print-line', `vn-finance · painel · exercício ${meta.year} · ${ptDate(meta.today)} · dados apenas neste computador`);

  const dir = document.getElementById('chip-dir');
  if (dir !== null) {
    dir.title = String(meta.dataDir ?? '');
    dir.innerHTML = `Pasta de dados: <span class="mono">${esc(meta.dataDir)}</span>`;
  }

  const nifNode = document.getElementById('nif-val');
  const reveal = document.getElementById('reveal-btn');
  const revealText = document.getElementById('reveal-txt');
  if (nifNode !== null && reveal !== null && revealText !== null) {
    const nif = profile === null ? '' : String(profile.nif ?? '');
    if (nif === '') {
      nifNode.textContent = '—';
      nifNode.className = 'nif';
      nifNode.removeAttribute('data-full');
      nifNode.removeAttribute('data-mask');
      reveal.disabled = true;
      reveal.setAttribute('aria-pressed', 'false');
      revealText.textContent = 'sem perfil';
    } else {
      nifNode.dataset.full = nif;
      nifNode.dataset.mask = maskNif(nif);
      nifNode.textContent = state.nifRevealed ? nif : nifNode.dataset.mask;
      nifNode.className = state.nifRevealed ? 'nif is-revealed' : 'nif';
      reveal.disabled = false;
      reveal.setAttribute('aria-pressed', state.nifRevealed ? 'true' : 'false');
      revealText.textContent = state.nifRevealed ? 'ocultar' : 'revelar';
    }
  }

  const badge = document.getElementById('rules-badge');
  if (badge !== null) {
    const verified = summary.verified ?? 0;
    const partial = summary.partial ?? 0;
    const unchecked = (summary.unverified ?? 0) + (summary.stale ?? 0);
    badge.innerHTML =
      '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M8 3.6S6.5 2 3 2v11c3.5 0 5 1.6 5 1.6S9.5 13 13 13V2c-3.5 0-5 1.6-5 1.6z"/></svg>' +
      `<span>Regras <span class="mono">pt/${esc(meta.packVersion)}</span> · ${verified} verificadas · ` +
      `${partial} parciais · ${unchecked} por verificar · ` +
      `${summary.officialSources ?? 0}/${summary.sources ?? 0} fontes oficiais</span>`;
  }

  const lastRetrieved = (model.rules?.sources ?? [])
    .map((source) => source.retrievedAt)
    .filter((value) => typeof value === 'string' && value !== '')
    .sort()
    .pop();
  setText('foot-rules', `Pacote de regras pt/${meta.packVersion} · verificação ${(summary.verified ?? 0)}/${summary.obligations ?? 0} regras`);
  setText(
    'foot-sources',
    lastRetrieved === undefined ? 'Fontes —' : `Fontes recolhidas em ${ptDate(lastRetrieved)} · ${(model.rules?.sources ?? []).length} fontes`,
  );

  /* Contadores da navegação. */
  const agenda = model.agenda ?? [];
  const open = agenda.filter(isActionable);
  const overdue = agenda.filter((instance) => instance.status === 'overdue');
  const dueSoon = agenda.filter((instance) => instance.status === 'due_soon');
  setCount(
    'cnt-agenda',
    String(open.length),
    overdue.length > 0 ? 'due' : dueSoon.length > 0 ? 'soon' : '',
  );

  const year = String(meta.year);
  const invoices = (model.invoices ?? []).filter((invoice) => String(invoice.date ?? '').startsWith(year));
  setCount('cnt-recibos', String(invoices.length), invoices.length === 0 ? 'off' : '');

  const ivaOpen = open.filter((instance) => String(instance.tax ?? '').toUpperCase().includes('IVA'));
  const irsOpen = open.filter((instance) => String(instance.tax ?? '').toUpperCase().includes('IRS'));
  setCount('cnt-iva', String(ivaOpen.length), ivaOpen.some((i) => i.status === 'overdue') ? 'due' : ivaOpen.length === 0 ? 'off' : 'soon');
  setCount('cnt-irs', String(irsOpen.length), irsOpen.some((i) => i.status === 'overdue') ? 'due' : irsOpen.length === 0 ? 'off' : 'soon');

  const documented = documentedRules(model);
  const missingDocs = open.filter((instance) => needsDocumentation(instance) && !documented.has(instance.ruleId));
  setCount('cnt-cofre', String(missingDocs.length), missingDocs.length > 0 ? 'due' : 'off');

  setCount('cnt-regras', String(summary.obligations ?? 0), (model.pack?.problems ?? []).length > 0 ? 'soon' : '');

  const update = model.update ?? {};
  if (update.hasKey === false) setCount('cnt-assistente', 'sem chave', 'off');
  else if (update.pending) setCount('cnt-assistente', `${update.pendingChanges ?? 0} por aplicar`, 'soon');
  else setCount('cnt-assistente', 'pronta', 'off');

  const meta2 = document.querySelector('meta[name="description"]');
  if (meta2 !== null) meta2.setAttribute('content', `vn-finance — painel local do exercício ${meta.year}. Alertas: ${counts.total}.`);
}

/* --------------------------------------------------------------------------
   2. Faixa de alerta + alertas fiscais
   -------------------------------------------------------------------------- */

function renderAlertBlock(model) {
  const today = model.meta.today;
  const flags = model.flags ?? [];
  const summary = model.flagSummary ?? { urgent: 0, attention: 0, info: 0, total: 0 };
  const agenda = model.agenda ?? [];

  const next30 = agenda.filter((instance) => {
    if (!isActionable(instance) || instance.status === 'overdue') return false;
    const delta = daysBetween(today, instance.dueDate);
    return delta >= 0 && delta <= 30;
  });
  const overdue = agenda.filter((instance) => instance.status === 'overdue');
  const documented = documentedRules(model);
  const missingDocs = agenda.filter(
    (instance) => isActionable(instance) && needsDocumentation(instance) && !documented.has(instance.ruleId),
  );

  const top = flags.find((flag) => flag.severity === 'urgent') ?? flags[0] ?? null;
  const counts =
    `<span>${next30.length} ${next30.length === 1 ? 'obrigação' : 'obrigações'} nos próximos 30 dias</span>` +
    (overdue.length > 0 ? `<span class="dot" aria-hidden="true">·</span><span>${overdue.length} em atraso</span>` : '') +
    `<span class="dot" aria-hidden="true">·</span><span>${missingDocs.length} ${missingDocs.length === 1 ? 'documento em falta' : 'documentos em falta'} no cofre</span>` +
    `<span class="dot" aria-hidden="true">·</span><span>${summary.urgent} urgentes · ${summary.attention} de atenção · ${summary.info} informativos</span>`;

  let strip;
  if (top === null) {
    strip =
      '<div class="alertbar pos">' +
      '<span class="pill p-ok"><span class="g" aria-hidden="true">✓</span>Sem alertas</span>' +
      '<div class="ab-body">' +
      '<p class="ab-title">Sem alertas fiscais ativos</p>' +
      `<p class="ab-meta">${counts}</p>` +
      '</div>' +
      '<div class="ab-actions"><button type="button" class="btn btn-primary" data-act="scroll-agenda">Ver o plano</button></div>' +
      '</div>';
  } else {
    const severity =
      top.severity === 'urgent'
        ? { cls: 'sev-alta', glyph: '▲', label: 'Alta' }
        : top.severity === 'attention'
          ? { cls: 'sev-media', glyph: '●', label: 'Média' }
          : { cls: 'sev-info', glyph: '○', label: 'Informativa' };
    strip =
      `<div class="alertbar${top.severity === 'info' ? ' neutral' : ''}">` +
      `<span class="sev ${severity.cls}"><span class="g" aria-hidden="true">${severity.glyph}</span> ${severity.label}</span>` +
      '<div class="ab-body">' +
      `<p class="ab-title">${esc(top.title)}</p>` +
      `<p class="ab-meta"><span>${escLines(top.detail)}</span></p>` +
      `<p class="ab-meta">${counts}</p>` +
      '</div>' +
      '<div class="ab-actions"><button type="button" class="btn btn-primary" data-act="scroll-agenda">Ver o plano</button></div>' +
      '</div>';
  }

  let list = '';
  if (flags.length > 0) {
    const rows = flags
      .map((flag) => {
        const severity =
          flag.severity === 'urgent'
            ? { cls: 'sev-alta', glyph: '▲', label: 'Alta' }
            : flag.severity === 'attention'
              ? { cls: 'sev-media', glyph: '●', label: 'Média' }
              : { cls: 'sev-info', glyph: '○', label: 'Informativa' };
        const links = [];
        if ((flag.invoiceNumbers ?? []).length > 0) links.push('<a href="#recibos">ver recibos</a>');
        if ((flag.ruleIds ?? []).length > 0) links.push('<a href="#agenda">ver agenda</a>');
        if ((flag.sourceIds ?? []).length > 0) links.push('<a href="#regras">ver fontes</a>');
        const basis = (flag.legalBasis ?? []).map((item) => `<span class="mono">${esc(item)}</span>`).join('<span aria-hidden="true"> · </span>');
        const sources = (flag.sourceIds ?? []).map((item) => `<span class="mono">${esc(item)}</span>`).join('<span aria-hidden="true"> · </span>');
        return (
          '<li class="alert">' +
          `<span class="sev ${severity.cls}"><span class="g" aria-hidden="true">${severity.glyph}</span> ${severity.label}</span>` +
          '<div>' +
          `<p class="a-t">${esc(flag.title)}</p>` +
          `<p class="a-d">${escLines(flag.detail)}</p>` +
          '<p class="a-src">' +
          (basis === '' ? '' : basis) +
          (basis !== '' && sources !== '' ? '<span aria-hidden="true">·</span>' : '') +
          (sources === '' ? '' : sources) +
          `<span class="badge">${esc(flag.code)}</span>` +
          ((flag.invoiceNumbers ?? []).length > 0
            ? `<span>documentos: <span class="mono">${esc(flag.invoiceNumbers.join(', '))}</span></span>`
            : '') +
          '</p>' +
          '</div>' +
          `<div class="why">${links.join('<br>')}</div>` +
          '</li>'
        );
      })
      .join('');
    list =
      '<section class="block" aria-label="Alertas do sistema fiscal">' +
      sectionHead(
        'Alertas do sistema fiscal',
        `${summary.total} verificações ativas · motor de regras pt/${esc(model.meta.packVersion)} · executado localmente, sem ligação à internet`,
      ) +
      `<ul class="alerts">${rows}</ul>` +
      '</section>';
  }

  return `<section class="block" aria-label="Faixa de alerta">${strip}</section>${list}`;
}

/* --------------------------------------------------------------------------
   3. Indicadores
   -------------------------------------------------------------------------- */

function renderKpis(model) {
  const meta = model.meta;
  const quarters = model.quarters ?? [];
  const reserve = model.reserve ?? { lowCents: 0, highCents: 0, basis: [], limitations: [] };
  const invoices = (model.invoices ?? []).filter((invoice) => String(invoice.date ?? '').startsWith(String(meta.year)));

  const sum = (pick) => quarters.reduce((total, quarter) => total + (Number(pick(quarter)) || 0), 0);
  const gross = sum((quarter) => quarter.totals?.baseCents);
  const retained = sum((quarter) => quarter.totals?.retencaoSofridaCents);
  const iva = sum((quarter) => quarter.totals?.ivaLiquidadoCents);
  const count = sum((quarter) => quarter.totals?.invoiceCount);

  const month = Number(String(meta.today).slice(5, 7));
  const currentQuarter = Math.min(4, Math.max(1, Math.ceil(month / 3)));
  const quarter = quarters.find((item) => item.quarter === currentQuarter) ?? null;
  const ss = quarter?.socialSecurity ?? null;

  const tiles = [
    {
      k: `Rendimento bruto ${meta.year}`,
      v: eur(gross),
      s:
        count === 0
          ? 'Sem recibos registados neste exercício'
          : 'Base de incidência dos recibos emitidos no exercício',
      f: `<span class="mono">${count} ${count === 1 ? 'documento' : 'documentos'}</span>`,
    },
    {
      k: 'Retenções na fonte',
      v: eur(retained),
      s: 'Retenção sofrida nos recibos do exercício, somada dos quatro trimestres',
      f: `<span class="mono">soma dos quatro trimestres</span>`,
    },
    {
      k: 'IVA liquidado (acum.)',
      v: eur(iva),
      s: 'IVA liquidado nos recibos do exercício, somado dos quatro trimestres',
      f: `<span class="mono">regime: ${esc(ivaRegimeLabel(model.profile?.iva?.regime))}</span>`,
    },
    {
      k: `Contribuições SS (T${currentQuarter})`,
      v: ss === null ? '—' : eur(ss.contributionCents),
      s:
        ss === null
          ? 'Sem relatório para o trimestre em curso'
          : `${bpPercent(ss.relevantIncomeShareBp)} do rendimento de serviços, à taxa de ${bpPercent(ss.contributionRateBp)}`,
      f: quarter === null ? '' : `<span class="mono">${esc(quarter.quarter)}.º trimestre · ${ptDate(quarter.start)}–${ptDate(quarter.end)}</span>`,
    },
    {
      k: `Reserva recomendada`,
      v: `${eur(reserve.lowCents)} – ${eur(reserve.highCents)}`,
      s: 'Intervalo indicativo a reservar para IRS e Segurança Social do rendimento do exercício',
      f: `<span class="badge est">estimativa</span><span class="mono">regras pt/${esc(meta.packVersion)}</span>`,
    },
  ];

  const html = tiles
    .map(
      (tile) =>
        '<div class="kpi">' +
        `<span class="kpi-k">${esc(tile.k)}</span>` +
        `<span class="kpi-v">${tile.v}</span>` +
        `<span class="kpi-s">${tile.s}</span>` +
        (tile.f === '' ? '' : `<span class="kpi-f">${tile.f}</span>`) +
        '</div>',
    )
    .join('');

  return (
    '<section class="block" aria-label="Indicadores do exercício">' +
    sectionHead(
      'Indicadores do exercício',
      `Acumulado a ${ptDate(meta.today)} · base: faturação registada no cofre local`,
    ) +
    `<div class="kpis">${html}</div>` +
    (invoices.length === 0
      ? '<p class="tnote">Ainda não há recibos registados neste exercício. Os indicadores mostram zero porque não há faturação, não porque falte um cálculo.</p>'
      : '') +
    '</section>'
  );
}

/* --------------------------------------------------------------------------
   4. Enquadramento (CAE e CIRS) + formulário dos dados em falta
   -------------------------------------------------------------------------- */

function renderEnquadramento(model) {
  const profile = model.profile;
  const activity = profile.activity ?? {};
  const defs = [];

  for (const cae of activity.cae ?? []) {
    defs.push({
      dt: cae.role === 'principal' ? 'CAE principal' : 'CAE secundário',
      dd: `<span class="mono">${esc(cae.code)}</span> — ${esc(cae.description)}`,
    });
  }
  if ((activity.cae ?? []).length === 0) {
    defs.push({ dt: 'CAE', dd: 'Nenhum CAE registado no perfil.' });
  }

  defs.push({
    dt: 'CIRS',
    dd:
      activity.categoryB === true
        ? 'Categoria B — rendimentos empresariais e profissionais'
        : 'Sem atividade aberta na categoria B registada no perfil',
  });

  defs.push({
    dt: 'Início de atividade',
    dd:
      `${ptDate(activity.startDate)}` +
      (profile.trackingStart ? `<small>Acompanhamento registado a partir de ${ptDate(profile.trackingStart)}</small>` : ''),
  });

  defs.push({
    dt: 'Regime IRS',
    dd:
      `${esc(irsRegimeLabel(profile.irs?.regime))}` +
      `<small>Coeficiente registado no perfil: <span class="mono">${bpCoefficient(profile.irs?.coefficientBp)}</span></small>`,
  });

  defs.push({
    dt: 'IVA',
    dd:
      `${esc(ivaRegimeLabel(profile.iva?.regime))}` +
      `<small>Regime de IVA registado no perfil · alterações são declarativas</small>`,
  });

  const yearInvoices = (model.invoices ?? []).filter((invoice) => String(invoice.date ?? '').startsWith(String(model.meta.year)));
  const ptRates = [...new Set(yearInvoices.filter((i) => i.clientCountry === 'PT').map((i) => i.retentionBp))];
  const foreign = yearInvoices.filter((i) => i.clientCountry !== 'PT');
  defs.push({
    dt: 'Retenção na fonte',
    dd:
      yearInvoices.length === 0
        ? 'Sem recibos registados neste exercício.'
        : `${ptRates.length === 0 ? 'Sem retenção registada em clientes residentes' : `Taxas registadas em clientes residentes: <span class="mono">${ptRates.map((bp) => bpPercent(bp)).join(' · ')}</span>`}` +
          `<small>${foreign.length} ${foreign.length === 1 ? 'recibo' : 'recibos'} a clientes não residentes neste exercício</small>`,
  });

  const quarter = (model.quarters ?? [])[0] ?? null;
  const ss = quarter?.socialSecurity ?? null;
  defs.push({
    dt: 'Segurança Social',
    dd:
      ss === null
        ? 'Sem relatório de contribuições no modelo.'
        : `<span class="mono">${bpPercent(ss.contributionRateBp)}</span> sobre <span class="mono">${bpPercent(ss.relevantIncomeShareBp)}</span> do rendimento de serviços` +
          `<small>Valores do pacote de regras, aplicados pelo motor local</small>`,
  });

  defs.push({
    dt: 'Volume de negócios declarado',
    dd:
      (activity.turnoverPreviousYearCents === undefined
        ? '<span class="badge miss">ano anterior em falta</span>'
        : `Ano anterior: <span class="mono">${eur(activity.turnoverPreviousYearCents)}</span>`) +
      '<small>' +
      (activity.turnoverCurrentYearExpectedCents === undefined
        ? '<span class="badge miss">previsão do ano corrente em falta</span>'
        : `Previsão para o ano corrente: <span class="mono">${eur(activity.turnoverCurrentYearExpectedCents)}</span>`) +
      '</small>',
  });

  const defsHtml = defs
    .map((def) => `<div class="def"><dt>${esc(def.dt)}</dt><dd>${def.dd}</dd></div>`)
    .join('');

  const problems = (model.profileProblems ?? [])
    .map((problem) => {
      const error = problem.level === 'error';
      return (
        `<div class="disc${error ? ' err' : ''}">` +
        `<span class="sev ${error ? 'sev-alta' : 'sev-media'}"><span class="g" aria-hidden="true">${error ? '▲' : '●'}</span> ${error ? 'Erro' : 'Aviso'}</span>` +
        `<div><p class="a-t">${esc(problem.field)}</p><p class="a-d">${esc(problem.message)}</p></div>` +
        '</div>'
      );
    })
    .join('');

  const missing = model.missingInputs ?? [];
  let form = '';
  if (missing.length > 0) {
    const regime = profile.iva?.regime ?? 'trimestral';
    const options = [
      { value: 'isento_art53', label: 'Isenção do art. 53.º do CIVA' },
      { value: 'trimestral', label: 'Regime normal — declaração trimestral' },
      { value: 'mensal', label: 'Regime normal — declaração mensal' },
    ]
      .map(
        (option) =>
          `<option value="${esc(option.value)}"${option.value === regime ? ' selected' : ''}>${esc(option.label)}</option>`,
      )
      .join('');

    form =
      '<div class="form noprint mt-12">' +
      '<p class="subhead">Dados que faltam para o motor de regras poder decidir</p>' +
      '<p class="tnote flush">' +
      'A aplicação recusa-se a inventar estes valores: sem eles não avalia a isenção do art. 53.º do CIVA nem avisa quando o limite se aproxima.' +
      '</p>' +
      '<form data-form="profile" novalidate>' +
      '<div class="form-grid">' +
      '<div class="field"><label for="p-prev">Volume de negócios do ano anterior (€)</label>' +
      `<input id="p-prev" name="turnoverPreviousYearCents" type="text" inputmode="decimal" autocomplete="off" value="${esc(centsToInput(activity.turnoverPreviousYearCents))}" placeholder="12 400,00">` +
      '<small>Em território nacional, no ano civil anterior, em euros.</small></div>' +
      '<div class="field"><label for="p-curr">Volume de negócios esperado para este ano (€)</label>' +
      `<input id="p-curr" name="turnoverCurrentYearExpectedCents" type="text" inputmode="decimal" autocomplete="off" value="${esc(centsToInput(activity.turnoverCurrentYearExpectedCents))}" placeholder="48 600,00">` +
      '<small>A tua estimativa. É o que permite avisar a meio do ano.</small></div>' +
      '<div class="field"><label for="p-iva">Regime de IVA</label>' +
      `<select id="p-iva" name="ivaRegime">${options}</select>` +
      '<small>Alterar o enquadramento é uma decisão declarativa, não um ajuste de cálculo.</small></div>' +
      '<div class="field"><span class="lbl">Clientes fora de Portugal</span>' +
      `<label class="check" for="p-eu"><input id="p-eu" name="intraCommunityOperations" type="checkbox"${activity.intraCommunityOperations === true ? ' checked' : ''}> <span>Presto serviços a clientes da União Europeia (operações intracomunitárias)</span></label>` +
      `<label class="check" for="p-ex"><input id="p-ex" name="exports" type="checkbox"${activity.exports === true ? ' checked' : ''}> <span>Presto serviços a clientes fora da União Europeia (exportações)</span></label>` +
      '</div>' +
      '</div>' +
      '<div class="form-actions">' +
      '<button type="submit" class="btn btn-primary">Guardar enquadramento</button>' +
      '<span class="note">Estes valores são gravados no cofre local. Nada é enviado para fora deste computador.</span>' +
      '</div>' +
      '<div data-role="form-message"></div>' +
      '</form>' +
      '</div>';
  }

  const missingList =
    missing.length === 0
      ? ''
      : '<div class="notes">' +
        missing
          .map((item) => `<p><span class="g" aria-hidden="true">!</span><span>${esc(item)}</span></p>`)
          .join('') +
        '<p><span class="g" aria-hidden="true">!</span><span>A data de início de atividade, se estiver em falta, define-se com <span class="cmd">vnfin init</span>.</span></p>' +
        '</div>';

  return (
    '<section class="block" id="enquadramento" aria-label="Enquadramento fiscal">' +
    sectionHead(
      'Enquadramento fiscal (CAE e CIRS)',
      `Cartão de identidade da atividade · início em ${ptDate(activity.startDate)}`,
    ) +
    `<dl class="defs">${defsHtml}</dl>` +
    problems +
    missingList +
    form +
    '</section>'
  );
}

/* --------------------------------------------------------------------------
   5. Agenda fiscal
   -------------------------------------------------------------------------- */

function filterAgenda(model) {
  const today = model.meta.today;
  const agenda = model.agenda ?? [];
  switch (state.filter) {
    case '30':
      return agenda.filter((instance) => {
        if (instance.status === 'overdue') return true;
        if (!isActionable(instance)) return false;
        const delta = daysBetween(today, instance.dueDate);
        return delta >= 0 && delta <= 30;
      });
    case '90':
      return agenda.filter((instance) => {
        if (instance.status === 'overdue') return true;
        if (!isActionable(instance)) return false;
        const delta = daysBetween(today, instance.dueDate);
        return delta >= 0 && delta <= 90;
      });
    case 'done':
      return agenda.filter((instance) => instance.status === 'done');
    case 'na':
      return agenda.filter((instance) => instance.status === 'not_applicable');
    default:
      return agenda;
  }
}

function filterCounts(model) {
  const today = model.meta.today;
  const agenda = model.agenda ?? [];
  const within = (days) =>
    agenda.filter((instance) => {
      if (instance.status === 'overdue') return true;
      if (!isActionable(instance)) return false;
      const delta = daysBetween(today, instance.dueDate);
      return delta >= 0 && delta <= days;
    }).length;
  return {
    '30': within(30),
    '90': within(90),
    year: agenda.length,
    done: agenda.filter((instance) => instance.status === 'done').length,
    na: agenda.filter((instance) => instance.status === 'not_applicable').length,
  };
}

function renderAgenda(model) {
  const today = model.meta.today;
  const counts = filterCounts(model);
  const list = filterAgenda(model);
  const documented = documentedRules(model);
  const total = (model.agenda ?? []).length;

  const buttons = FILTERS.map((filter) => {
    const on = state.filter === filter.id;
    return (
      `<button type="button" data-act="filter" data-filter="${esc(filter.id)}" ` +
      `aria-pressed="${on ? 'true' : 'false'}" class="${on ? 'on' : ''}">` +
      `${esc(filter.label)} <b class="c">${counts[filter.id]}</b></button>`
    );
  }).join('');

  const legend =
    '<span class="legend" aria-label="Legenda de estados">' +
    '<i><span class="pill p-ok"><span class="g" aria-hidden="true">✓</span>Concluído</span></i>' +
    '<i><span class="pill p-warn"><span class="g" aria-hidden="true">●</span>Pendente</span></i>' +
    '<i><span class="pill p-danger"><span class="g" aria-hidden="true">▲</span>Vence hoje ou amanhã</span></i>' +
    '<i><span class="pill p-danger"><span class="g" aria-hidden="true">▲</span>Em atraso</span></i>' +
    '<i><span class="pill p-future"><span class="g" aria-hidden="true">○</span>Futuro</span></i>' +
    '<i><span class="pill p-future"><span class="g" aria-hidden="true">—</span>Histórico</span></i>' +
    '<i><span class="pill p-na"><span class="g" aria-hidden="true">⊘</span>Não aplicável</span></i>' +
    '</span>';

  const toolbar =
    '<div class="toolbar noprint">' +
    `<span class="seg" role="group" aria-label="Filtro da agenda">${buttons}</span>` +
    legend +
    '</div>';

  if (list.length === 0) {
    const message =
      total === 0
        ? emptyState('Agenda vazia', 'O motor de regras não devolveu obrigações para este exercício. Nada registado — e nada inventado.')
        : emptyState('Nada neste filtro', `Existem ${total} obrigações na agenda, mas nenhuma corresponde ao filtro escolhido.`);
    return (
      '<section class="block" id="agenda" aria-label="Agenda fiscal">' +
      sectionHead(`Agenda fiscal ${model.meta.year}`, `${total} obrigações · motor de regras pt/${esc(model.meta.packVersion)}`) +
      toolbar +
      message +
      '</section>'
    );
  }

  const byMonth = new Map();
  for (const instance of list) {
    const key = instance.dueDate.slice(0, 7);
    byMonth.set(key, (byMonth.get(key) ?? 0) + 1);
  }

  const rows = [];
  let lastMonth = '';
  let todayDone = false;

  for (const instance of list) {
    const month = instance.dueDate.slice(0, 7);
    if (month !== lastMonth) {
      lastMonth = month;
      const n = byMonth.get(month) ?? 0;
      rows.push(`<tr class="grp"><td colspan="7">${esc(monthLabel(instance.dueDate))} · ${n} ${n === 1 ? 'obrigação' : 'obrigações'}</td></tr>`);
    }
    if (!todayDone && instance.dueDate >= today) {
      rows.push(`<tr class="today"><td colspan="7">${ptDate(today)} — hoje · ${esc(weekdayLabel(today))}</td></tr>`);
      todayDone = true;
    }
    rows.push(agendaRow(instance, model, documented));
  }

  const openCount = (model.agenda ?? []).filter(isActionable).length;
  const doneCount = (model.agenda ?? []).filter((instance) => instance.status === 'done').length;

  return (
    '<section class="block" id="agenda" aria-label="Agenda fiscal">' +
    sectionHead(
      `Agenda fiscal ${model.meta.year}`,
      `${total} obrigações · ${doneCount} concluídas · ${openCount} em aberto · prazos do pacote de regras pt/${esc(model.meta.packVersion)}`,
    ) +
    toolbar +
    tableWrap(
      'Agenda fiscal: data de vencimento, obrigação, tipo, período, estado, origem da data e ação.',
      '<th scope="col">Data</th><th scope="col">Obrigação</th><th scope="col">Tipo</th><th scope="col">Período</th>' +
        '<th scope="col">Estado</th><th scope="col">Fonte</th><th scope="col">Ação</th>',
      rows.join(''),
    ) +
    '<p class="tnote">A fonte indica de onde veio a data: <span class="mono">oficial</span> quando é a data publicada pela autoridade, ' +
    '<span class="mono">calculada</span> quando resulta da regra geral, <span class="mono">provisória</span> quando o pacote de regras ' +
    'ainda não cobre este ano. Os filtros de 30 e 90 dias incluem sempre as obrigações em atraso. ' +
    'As linhas marcadas como histórico são anteriores ao início do acompanhamento e não são falhas.</p>' +
    '</section>'
  );
}

function agendaRow(instance, model, documented) {
  const pill = statusPill(instance, model.meta.today);
  const fonte = fonteBadge(instance);

  const notesOut = [];
  if (instance.status === 'not_applicable' && instance.notApplicableReason) {
    notesOut.push(`<span class="ob-note"><span class="g" aria-hidden="true">⊘</span> ${esc(instance.notApplicableReason)}</span>`);
  }
  for (const discrepancy of instance.discrepancies ?? []) {
    notesOut.push(`<span class="ob-note"><span class="g" aria-hidden="true">▲</span> ${esc(discrepancy)}</span>`);
  }
  if (instance.verifyNote) {
    notesOut.push(`<span class="ob-note q"><span class="g" aria-hidden="true">?</span> ${esc(instance.verifyNote)}</span>`);
  }
  if (isActionable(instance) && needsDocumentation(instance) && !documented.has(instance.ruleId)) {
    notesOut.push(
      `<span class="ob-note"><span class="g" aria-hidden="true">▲</span> Sem documentação de suporte no cofre (${instance.documentsToKeep.length})</span>`,
    );
  }
  if (instance.penaltyNote && instance.status !== 'not_applicable' && instance.status !== 'untracked') {
    notesOut.push(`<span class="ob-note q">${esc(instance.penaltyNote)}</span>`);
  }

  const legal = (instance.legalBasis ?? []).map((item) => esc(item)).join(' · ');
  const sources = (instance.sourceIds ?? []).map((item) => esc(item)).join(' · ');

  let action;
  if (instance.status === 'done') {
    action = '<span class="mini">✓ tratado</span>';
  } else if (instance.status === 'not_applicable') {
    action = '<a href="#enquadramento">Porquê</a>';
  } else if (instance.status === 'untracked') {
    action = '<span class="mini">— histórico</span>';
  } else {
    const style = instance.status === 'overdue' || instance.status === 'due_soon' ? 'btn btn-primary btn-sm' : 'btn btn-sm';
    action =
      `<button type="button" class="${style}" data-act="complete" data-id="${esc(instance.id)}" ` +
      `data-rule="${esc(instance.ruleId)}" data-due="${esc(instance.dueDate)}">Marcar como tratado</button>`;
  }

  return (
    `<tr${instance.status === 'not_applicable' ? ' class="na"' : ''}>` +
    `<td class="d">${ptDate(instance.dueDate)}</td>` +
    '<td><span class="ob">' +
    esc(instance.title) +
    '</span>' +
    notesOut.join('') +
    (legal === '' ? '' : `<span class="ob-src">${legal}</span>`) +
    (sources === '' ? '' : `<span class="ob-src">fontes: ${sources}</span>`) +
    '</td>' +
    `<td><span class="tipo">${esc(KIND_LABEL[instance.kind] ?? instance.kind)}</span></td>` +
    `<td class="small">${esc(instance.periodLabel)}</td>` +
    `<td><span class="pill ${pill.cls}"><span class="g" aria-hidden="true">${pill.glyph}</span>${esc(pill.label)}</span></td>` +
    `<td><span class="${fonte.cls}">${esc(fonte.text)}</span></td>` +
    `<td class="act">${action}</td>` +
    '</tr>'
  );
}

/* --------------------------------------------------------------------------
   6. Recibos emitidos
   -------------------------------------------------------------------------- */

/** Espelha `applyBp` do núcleo: meios cêntimos arredondam para cima. */
function applyBp(amount, bp) {
  const value = (Number(amount) * Number(bp)) / 10000;
  return value >= 0 ? Math.floor(value + 0.5) : Math.ceil(value - 0.5);
}

function invoiceTotals(invoice) {
  const iva = applyBp(invoice.baseCents, invoice.ivaRateBp);
  const retention = applyBp(invoice.baseCents, invoice.retentionBp);
  return { iva, retention, net: invoice.baseCents + iva - retention };
}

function renderRecibos(model) {
  const meta = model.meta;
  const year = String(meta.year);
  const all = model.invoices ?? [];
  const invoices = all.filter((invoice) => String(invoice.date ?? '').startsWith(year));
  const others = all.length - invoices.length;

  const tool =
    '<div class="tool noprint">' +
    '<span class="tool-r">' +
    '<button type="button" class="btn btn-primary" data-act="open-invoice">Nova fatura-recibo</button>' +
    '</span>' +
    '<span class="legend" aria-label="Origem das colunas">' +
    '<i>IVA, retenção e líquido derivados das taxas registadas em cada recibo</i>' +
    '</span>' +
    '</div>';

  const empty =
    invoices.length === 0
      ? emptyState(
          'Nada registado',
          all.length === 0
            ? 'Não há recibos no cofre local. Regista o primeiro com o formulário abaixo.'
            : `Não há recibos de ${year}. Existem ${others} recibos de outros exercícios no cofre.`,
        )
      : '';

  let table = '';
  if (invoices.length > 0) {
    let base = 0;
    let iva = 0;
    let retention = 0;
    let net = 0;

    const rows = invoices
      .map((invoice) => {
        const totals = invoiceTotals(invoice);
        base += invoice.baseCents;
        iva += totals.iva;
        retention += totals.retention;
        net += totals.net;
        const status = INVOICE_STATUS[invoice.status] ?? { cls: 'p-future', glyph: '○', label: String(invoice.status ?? '—') };
        const foreign = invoice.clientCountry !== 'PT';
        return (
          '<tr>' +
          `<td class="d">${esc(invoice.number)}</td>` +
          `<td class="d">${ptDate(invoice.date)}</td>` +
          '<td class="cust"><span class="nm">' +
          esc(invoice.clientName) +
          '</span><span class="nf">' +
          (invoice.clientNif ? `NIF ${esc(invoice.clientNif)}` : 'sem NIF registado') +
          '</span>' +
          (foreign ? `<span class="badge">${esc(invoice.clientCountry)}</span>` : '') +
          (invoice.description ? `<span class="sub">${esc(invoice.description)}</span>` : '') +
          '</td>' +
          `<td class="n">${eur(invoice.baseCents)}</td>` +
          `<td class="n">${eur(totals.iva)}<span class="sub">${bpPercent(invoice.ivaRateBp)} · ${esc(TREATMENT_LABEL[invoice.vatTreatment] ?? invoice.vatTreatment)}</span></td>` +
          `<td class="n">${eur(totals.retention)}<span class="sub">${bpPercent(invoice.retentionBp)}</span></td>` +
          `<td class="n">${eur(totals.net)}</td>` +
          `<td><span class="pill ${status.cls}"><span class="g" aria-hidden="true">${status.glyph}</span>${esc(status.label)}</span>` +
          (invoice.atcud ? `<span class="sub mono">${esc(invoice.atcud)}</span>` : '') +
          '</td>' +
          '<td>' +
          (invoice.paymentProofInVault
            ? '<span class="pill p-ok"><span class="g" aria-hidden="true">✓</span>No cofre</span>'
            : '<span class="miss-tag"><span class="g" aria-hidden="true">▲</span>em falta</span>') +
          '</td>' +
          '</tr>'
        );
      })
      .join('');

    const totalsRow =
      '<tr class="tot">' +
      `<td colspan="3">Total · ${invoices.length} ${invoices.length === 1 ? 'documento' : 'documentos'}</td>` +
      `<td class="n">${eur(base)}</td>` +
      `<td class="n">${eur(iva)}</td>` +
      `<td class="n">${eur(retention)}</td>` +
      `<td class="n">${eur(net)}</td>` +
      '<td colspan="2">Derivado das taxas registadas em cada recibo</td>' +
      '</tr>';

    table = tableWrap(
      `Recibos emitidos em ${year}: número, data, cliente, base, IVA, retenção, líquido, estado e comprovativo.`,
      '<th scope="col">N.º</th><th scope="col">Data</th><th scope="col">Cliente</th>' +
        '<th scope="col" class="n">Base</th><th scope="col" class="n">IVA</th><th scope="col" class="n">Retenção</th>' +
        '<th scope="col" class="n">Líquido</th><th scope="col">Estado</th><th scope="col">Comprovativo</th>',
      rows + totalsRow,
    );
  }

  const footer =
    '<div class="tfoot">' +
    `<span>${invoices.length} recibos de ${year}${others > 0 ? ` · ${others} registos de outros exercícios não apresentados` : ''}</span>` +
    '<span>O líquido é <span class="mono">base + IVA − retenção</span>, calculado a partir das taxas registadas em cada recibo.</span>' +
    '</div>';

  return (
    '<section class="block" id="recibos" aria-label="Recibos emitidos">' +
    sectionHead(
      `Recibos emitidos (${year})`,
      invoices.length === 0
        ? 'Sem faturação registada neste exercício'
        : `${invoices.length} documentos · base ${eur(invoices.reduce((t, i) => t + i.baseCents, 0))}`,
    ) +
    tool +
    invoiceForm(model) +
    empty +
    table +
    footer +
    '</section>'
  );
}

function invoiceForm(model) {
  const meta = model.meta;
  const ptDefaults = state.ledger.pt ?? {};
  const treatmentDefault =
    model.profile?.iva?.regime === 'isento_art53' ? 'isento_art53' : 'iva_pt';

  const options = COUNTRIES.map(
    (country) =>
      `<option value="${esc(country.code)}" data-group="${esc(country.group)}"${country.code === 'PT' ? ' selected' : ''}>${esc(country.name)}</option>`,
  ).join('');

  const treatments = Object.entries(TREATMENT_LABEL)
    .map(
      ([value, label]) =>
        `<option value="${esc(value)}"${value === treatmentDefault ? ' selected' : ''}>${esc(label)}</option>`,
    )
    .join('');

  const statuses = [
    { value: 'issued', label: 'Emitido' },
    { value: 'paid', label: 'Pago' },
    { value: 'pending', label: 'Pendente' },
  ]
    .map(
      (status) =>
        `<option value="${esc(status.value)}"${status.value === 'issued' ? ' selected' : ''}>${esc(status.label)}</option>`,
    )
    .join('');

  return (
    '<details class="disclosure noprint" id="nova-fatura">' +
    '<summary>Nova fatura-recibo<span class="cnt off">registo local</span></summary>' +
    '<div class="d-body">' +
    '<form data-form="invoice" novalidate>' +
    '<div class="form-grid">' +
    '<div class="field"><label for="f-number">Número do documento</label>' +
    '<input id="f-number" name="number" type="text" autocomplete="off" placeholder="FR 2026/001">' +
    '<small>Deixa vazio para não fixar número.</small></div>' +
    '<div class="field"><label for="f-date">Data de emissão</label>' +
    `<input id="f-date" name="date" type="date" value="${esc(meta.today)}" required></div>` +
    '<div class="field"><label for="f-client">Nome do cliente</label>' +
    '<input id="f-client" name="clientName" type="text" autocomplete="off" required></div>' +
    '<div class="field"><label for="f-nif">NIF do cliente</label>' +
    '<input id="f-nif" name="clientNif" type="text" inputmode="numeric" autocomplete="off" placeholder="opcional"></div>' +
    '<div class="field"><label for="f-country">País do cliente</label>' +
    `<select id="f-country" name="clientCountry">${options}</select>` +
    '<small>Pré-seleciona o tratamento de IVA e a retenção a partir do grupo do país.</small></div>' +
    '<div class="field" id="f-country-other-wrap" hidden><label for="f-country-other">Código do país (ISO)</label>' +
    '<input id="f-country-other" name="clientCountryOther" type="text" autocomplete="off" maxlength="2" placeholder="XX"></div>' +
    '<div class="field full"><label for="f-desc">Descrição dos serviços</label>' +
    '<input id="f-desc" name="description" type="text" autocomplete="off" placeholder="opcional"></div>' +
    '<div class="field"><label for="f-base">Base de incidência (€)</label>' +
    '<input id="f-base" name="baseCents" type="text" inputmode="decimal" autocomplete="off" placeholder="1 200,50" required>' +
    '<small>Escreve em euros; o painel converte para cêntimos antes de enviar.</small></div>' +
    '<div class="field"><label for="f-iva">Taxa de IVA (%)</label>' +
    `<input id="f-iva" name="ivaRateBp" type="text" inputmode="decimal" autocomplete="off" value="${esc(bpToInput(ptDefaults.ivaBp))}" placeholder="23" required>` +
    '<small>Percentagem; o painel converte para pontos base.</small></div>' +
    '<div class="field"><label for="f-treatment">Tratamento de IVA</label>' +
    `<select id="f-treatment" name="vatTreatment">${treatments}</select></div>` +
    '<div class="field"><label for="f-ret">Retenção na fonte (%)</label>' +
    `<input id="f-ret" name="retentionBp" type="text" inputmode="decimal" autocomplete="off" value="${esc(bpToInput(ptDefaults.retentionBp))}" placeholder="25">` +
    '<small>Percentagem; zero significa sem retenção.</small></div>' +
    '<div class="field"><label for="f-atcud">ATCUD</label>' +
    '<input id="f-atcud" name="atcud" type="text" autocomplete="off" placeholder="opcional"></div>' +
    '<div class="field"><label for="f-status">Estado do pagamento</label>' +
    `<select id="f-status" name="status">${statuses}</select></div>` +
    '<div class="field"><span class="lbl">Comprovativo</span>' +
    '<label class="check" for="f-proof"><input id="f-proof" name="paymentProofInVault" type="checkbox"> <span>O comprovativo de recebimento já está no cofre</span></label>' +
    '</div>' +
    '</div>' +
    '<div class="form-actions">' +
    '<button type="submit" class="btn btn-primary">Registar recibo</button>' +
    '<span class="note">A pré-seleção de IVA e retenção vem do perfil e dos recibos já registados: confirma-a antes de gravar.</span>' +
    '</div>' +
    '<div data-role="form-message"></div>' +
    '</form>' +
    '</div>' +
    '</details>'
  );
}

/* --------------------------------------------------------------------------
   7. Segurança Social
   -------------------------------------------------------------------------- */

function renderSs(model) {
  const month = Number(String(model.meta.today).slice(5, 7));
  const currentQuarter = Math.min(4, Math.max(1, Math.ceil(month / 3)));
  const quarters = model.quarters ?? [];
  const quarter = quarters.find((item) => item.quarter === currentQuarter) ?? quarters[0] ?? null;

  if (quarter === null) {
    return (
      '<section class="block" id="ss" aria-label="Segurança Social">' +
      sectionHead('Segurança Social', 'Sem relatório de trimestre no modelo') +
      emptyState('Nada registado', 'O modelo não incluiu qualquer relatório trimestral.') +
      '</section>'
    );
  }

  const ss = quarter.socialSecurity;
  const instalments = ss.instalmentsCents ?? [];
  const base = eur(ss.serviceIncomeCents);

  const chain =
    '<ol class="chain">' +
    '<li class="calc"><span class="calc-k">Rendimento de serviços do trimestre</span>' +
    `<span class="calc-v">${eur(ss.serviceIncomeCents)}</span>` +
    `<span class="calc-s">Faturação emitida entre ${ptDate(quarter.start)} e ${ptDate(quarter.end)} · base, sem IVA e sem retenção</span></li>` +
    `<li class="calc-op"><span class="opb">× ${bpPercent(ss.relevantIncomeShareBp)}</span><span>percentagem do rendimento de prestações de serviços que constitui rendimento relevante, segundo o pacote de regras</span></li>` +
    '<li class="calc sum"><span class="calc-k">Rendimento relevante</span>' +
    `<span class="calc-v">${eur(ss.relevantIncomeCents)}</span>` +
    `<span class="calc-s">${base} × ${bpCoefficient(ss.relevantIncomeShareBp)}</span></li>` +
    `<li class="calc-op"><span class="opb">× ${bpPercent(ss.contributionRateBp)}</span><span>taxa contributiva aplicável aos trabalhadores independentes</span></li>` +
    '<li class="calc total"><span class="calc-k">Contribuição do trimestre</span>' +
    `<span class="calc-v">${eur(ss.contributionCents)}</span>` +
    `<span class="calc-s">${eur(ss.relevantIncomeCents)} × ${bpCoefficient(ss.contributionRateBp)}</span></li>` +
    (instalments.length === 0
      ? ''
      : `<li class="calc-op"><span class="opb">÷ ${instalments.length}</span><span>prestações mensais do trimestre, pagas entre os dias 10 e 20 de cada mês</span></li>` +
        '<li class="calc"><span class="calc-k">Prestação mensal</span>' +
        `<span class="calc-v">${eur(instalments[0])}</span>` +
        '<span class="calc-s">O resto em cêntimos fica nas primeiras prestações, para a soma fechar exatamente a contribuição</span></li>') +
    '</ol>';

  const instalmentBlock =
    instalments.length === 0
      ? ''
      : '<div class="inst">' +
        `<div class="inst-row" aria-hidden="true">${instalments.map((_, index) => `<span>${index + 1}.ª prestação</span>`).join('')}</div>` +
        `<div class="inst-bar" aria-hidden="true">${instalments.map(() => '<i></i>').join('')}</div>` +
        `<div class="inst-row">${instalments.map((value) => `<span>${eur(value)}</span>`).join('')}</div>` +
        '</div>';

  const side =
    '<div class="ss-side">' +
    '<h3>Notas do cálculo</h3>' +
    '<dl>' +
    `<dt class="first">Trimestre</dt><dd>${esc(quarter.quarter)}.º trimestre de ${esc(quarter.year)} · <span class="mono">${ptDate(quarter.start)}–${ptDate(quarter.end)}</span></dd>` +
    `<dt>Documentos considerados</dt><dd>${esc(quarter.totals?.invoiceCount ?? 0)} recibos do trimestre</dd>` +
    '<dt>Base de incidência</dt><dd>Rendimento de serviços faturado, sem IVA e sem retenção</dd>' +
    '<dt>Origem</dt><dd>Estimativa do motor local a partir do pacote de regras <span class="badge est">estimativa</span></dd>' +
    '</dl>' +
    notes(ss.limitations) +
    '</div>';

  return (
    '<section class="block" id="ss" aria-label="Segurança Social">' +
    sectionHead(
      `Segurança Social — ${quarter.quarter}.º trimestre ${quarter.year}`,
      'Cálculo da contribuição do trimestre, passo a passo, a partir da faturação registada',
    ) +
    `<div class="ss"><div>${chain}${instalmentBlock}</div>${side}</div>` +
    (quarter.totals?.invoiceCount === 0
      ? '<p class="tnote">Não há recibos com data neste trimestre: a contribuição apresentada é zero por ausência de faturação registada.</p>'
      : '') +
    '</section>'
  );
}

/* --------------------------------------------------------------------------
   8. IVA e IRS
   -------------------------------------------------------------------------- */

function renderIvaIrs(model) {
  const quarters = model.quarters ?? [];
  const reserve = model.reserve ?? { lowCents: 0, highCents: 0, basis: [], limitations: [] };

  if (quarters.length === 0) {
    return (
      '<section class="block" id="iva-irs" aria-label="IVA e IRS">' +
      sectionHead('IVA e IRS', 'Relatórios trimestrais do exercício') +
      emptyState('Nada registado', 'O modelo não incluiu relatórios trimestrais.') +
      '</section>'
    );
  }

  let base = 0;
  let iva = 0;
  let retention = 0;
  let invoiceCount = 0;
  let ss = 0;

  const rows = quarters
    .map((quarter) => {
      base += quarter.totals?.baseCents ?? 0;
      iva += quarter.totals?.ivaLiquidadoCents ?? 0;
      retention += quarter.totals?.retencaoSofridaCents ?? 0;
      invoiceCount += quarter.totals?.invoiceCount ?? 0;
      ss += quarter.socialSecurity?.contributionCents ?? 0;
      return (
        '<tr>' +
        `<td>${esc(quarter.quarter)}.º trimestre</td>` +
        `<td class="d">${ptDate(quarter.start)} – ${ptDate(quarter.end)}</td>` +
        `<td class="n">${esc(quarter.totals?.invoiceCount ?? 0)}</td>` +
        `<td class="n">${eur(quarter.totals?.baseCents ?? 0)}</td>` +
        `<td class="n">${eur(quarter.totals?.ivaLiquidadoCents ?? 0)}</td>` +
        `<td class="n">${eur(quarter.totals?.retencaoSofridaCents ?? 0)}</td>` +
        `<td class="n">${eur(quarter.socialSecurity?.contributionCents ?? 0)}</td>` +
        '</tr>'
      );
    })
    .join('');

  const totals =
    '<tr class="tot">' +
    '<td colspan="2">Total do exercício</td>' +
    `<td class="n">${invoiceCount}</td>` +
    `<td class="n">${eur(base)}</td>` +
    `<td class="n">${eur(iva)}</td>` +
    `<td class="n">${eur(retention)}</td>` +
    `<td class="n">${eur(ss)}</td>` +
    '</tr>';

  return (
    '<section class="block" id="iva-irs" aria-label="IVA e IRS">' +
    sectionHead(
      'IVA e IRS',
      `Relatórios trimestrais de ${model.meta.year} · o motor para no rendimento tributável, por desenho`,
    ) +
    tableWrap(
      'Relatórios trimestrais de IVA e Segurança Social, com base, IVA liquidado, retenção sofrida e contribuição estimada.',
      '<th scope="col">Trimestre</th><th scope="col">Período</th><th scope="col" class="n">Documentos</th>' +
        '<th scope="col" class="n">Base</th><th scope="col" class="n">IVA liquidado</th>' +
        '<th scope="col" class="n">Retenção sofrida</th><th scope="col" class="n">Contribuição SS</th>',
      rows + totals,
    ) +
    '<p class="tnote">Os valores de IVA e de retenção são somas dos recibos registados em cada trimestre. ' +
    'A contribuição da Segurança Social é a estimativa do motor local para o trimestre.</p>' +
    `<div class="panel mt-10 panel-pad">` +
    '<p class="subhead">Reserva recomendada</p>' +
    `<p class="dl-inline"><span class="kpi-v sm">${eur(reserve.lowCents)} – ${eur(reserve.highCents)}</span><span class="badge est">estimativa</span></p>` +
    ((reserve.basis ?? []).length === 0
      ? ''
      : `<p class="tnote">Base: ${(reserve.basis ?? []).map((item) => esc(item)).join(' · ')}</p>`) +
    notes(reserve.limitations) +
    '</div>' +
    '<p class="tnote">Não há, em lado nenhum deste painel, um valor final de IRS a pagar: o modelo para no rendimento tributável e as ' +
    'limitações acima dizem porquê. Qualquer apuramento final exige as taxas do ano e as deduções, e é matéria de um contabilista certificado.</p>' +
    '</section>'
  );
}

/* --------------------------------------------------------------------------
   9. Cofre de documentos
   -------------------------------------------------------------------------- */

function renderCofre(model) {
  const vault = model.vault ?? { entries: [], files: 0, bytes: 0 };
  const entries = vault.entries ?? [];
  const documented = documentedRules(model);
  const agenda = model.agenda ?? [];
  const missing = agenda.filter(
    (instance) => isActionable(instance) && needsDocumentation(instance) && !documented.has(instance.ruleId),
  );

  const head =
    '<div class="vault-head">' +
    (missing.length > 0
      ? `<span class="pill p-danger"><span class="g" aria-hidden="true">▲</span>${missing.length} em falta</span>`
      : '<span class="pill p-ok"><span class="g" aria-hidden="true">✓</span>Sem pendências conhecidas</span>') +
    '<span class="vault-sum">' +
    `<span>${esc(vault.files)} ${vault.files === 1 ? 'ficheiro' : 'ficheiros'} no cofre · ${bytesLabel(vault.bytes)}</span>` +
    `<span>Índice: ${entries.length} ${entries.length === 1 ? 'entrada' : 'entradas'}</span>` +
    `<span>Pasta local: <span class="mono">${esc(model.meta.dataDir)}</span></span>` +
    '</span>' +
    '</div>';

  const form =
    '<details class="disclosure noprint">' +
    '<summary>Registar documento por caminho absoluto<span class="cnt off">o servidor calcula o SHA-256 e copia</span></summary>' +
    '<div class="d-body">' +
    '<form data-form="document" novalidate>' +
    '<div class="form-grid">' +
    '<div class="field full"><label for="d-path">Caminho absoluto do ficheiro</label>' +
    '<input id="d-path" name="path" type="text" autocomplete="off" spellcheck="false" placeholder="C:\\Users\\...\\guia-iva-t3.pdf" required>' +
    '<small>O ficheiro é lido, identificado por hash e copiado para o cofre por este servidor local.</small></div>' +
    '<div class="field"><label for="d-kind">Tipo de documento</label>' +
    '<select id="d-kind" name="kind">' +
    '<option value="declaracao">Declaração</option>' +
    '<option value="guia">Guia de pagamento</option>' +
    '<option value="comprovativo">Comprovativo</option>' +
    '<option value="fatura">Fatura ou recibo</option>' +
    '<option value="saft">SAF-T</option>' +
    '<option value="contrato">Contrato</option>' +
    '<option value="outro" selected>Outro</option>' +
    '</select></div>' +
    '<div class="field"><label for="d-obligation">Obrigação associada (id da regra)</label>' +
    '<input id="d-obligation" name="obligationId" type="text" autocomplete="off" placeholder="opcional · ex. iva.dp.trimestral">' +
    '<small>Associar o documento à regra é o que faz o painel deixar de o contar como em falta.</small></div>' +
    '</div>' +
    '<div class="form-actions">' +
    '<button type="submit" class="btn btn-primary">Registar no cofre</button>' +
    '<span class="note">O painel não recebe o conteúdo do ficheiro: só o caminho, que o servidor local lê.</span>' +
    '</div>' +
    '<div data-role="form-message"></div>' +
    '</form>' +
    '</div>' +
    '</details>';

  let table = '';
  if (entries.length > 0) {
    const rows = entries
      .map((entry) => {
        const sha = String(entry.sha256 ?? '');
        return (
          '<tr>' +
          `<td><span class="ob">${esc(entry.file)}</span></td>` +
          `<td><span class="tipo">${esc(entry.kind)}</span></td>` +
          `<td class="d">${stamp(entry.addedAt)}</td>` +
          `<td class="pt">${entry.obligationId === null ? '—' : esc(entry.obligationId)}</td>` +
          `<td class="sha" title="${esc(sha)}">${sha === '' ? '—' : `${esc(sha.slice(0, 16))}…`}</td>` +
          '</tr>'
        );
      })
      .join('');
    table = tableWrap(
      'Documentos registados no cofre local, com tipo, data de registo, obrigação associada e impressão digital SHA-256.',
      '<th scope="col">Documento</th><th scope="col">Tipo</th><th scope="col">Registado</th>' +
        '<th scope="col">Obrigação</th><th scope="col">SHA-256</th>',
      rows,
    );
  } else {
    table = emptyState(
      'Nada registado no cofre',
      'Ainda não há documentos indexados. Regista um comprovativo pelo caminho absoluto para começar a fechar as pendências.',
    );
  }

  return (
    '<section class="block" id="cofre" aria-label="Cofre de documentos">' +
    sectionHead(
      'Cofre de documentos',
      `${entries.length} entradas indexadas · ficheiros guardados apenas nesta máquina`,
    ) +
    head +
    form +
    table +
    checklistTable(model, documented) +
    '<p class="tnote">O cofre é uma pasta local indexada com o hash de cada ficheiro. Nada é copiado para a nuvem e o painel nunca envia o conteúdo dos documentos para lado nenhum.</p>' +
    '</section>'
  );
}

/**
 * A lista de documentos a conservar, por obrigação por cumprir. A marca de cada
 * linha não é uma opinião: é o resultado de existir (ou não) um ficheiro no
 * cofre associado ao id daquela regra.
 */
function checklistTable(model, documented) {
  const instances = (model.agenda ?? []).filter(
    (instance) => isActionable(instance) && needsDocumentation(instance),
  );
  if (instances.length === 0) return '';

  const groups = new Map();
  for (const instance of instances) {
    const key = String(instance.tax ?? '') === '' ? 'Geral' : String(instance.tax);
    const bucket = groups.get(key) ?? [];
    bucket.push(instance);
    groups.set(key, bucket);
  }

  const rows = [];
  for (const [group, list] of groups) {
    rows.push(`<tr class="grp"><td colspan="4">${esc(group)}</td></tr>`);
    for (const instance of list) {
      const present = documented.has(instance.ruleId);
      rows.push(
        '<tr>' +
        '<td class="vc">' +
        `<span class="cb${present ? ' on' : ' miss'}" aria-hidden="true">${present ? '<svg viewBox="0 0 10 10"><path d="M1.6 5.2l2.4 2.6 4.4-5.6"/></svg>' : ''}</span>` +
        `<span class="sr-only">${present ? 'Documento presente no cofre' : 'Documento em falta no cofre'}</span>` +
        '</td>' +
        `<td><span class="ob">${esc(instance.title)}</span><span class="ob-src">${esc(instance.id)} · vence a ${ptDate(instance.dueDate)}</span></td>` +
        `<td>${(instance.documentsToKeep ?? []).map((item) => `<span class="ob-note">${esc(item)}</span>`).join('')}</td>` +
        `<td>${present ? '<span class="pill p-ok"><span class="g" aria-hidden="true">✓</span>No cofre</span>' : '<span class="pill p-danger"><span class="g" aria-hidden="true">▲</span>Em falta</span>'}</td>` +
        '</tr>',
      );
    }
  }

  return (
    '<p class="subhead mt-12">Documentos a conservar, por obrigação</p>' +
    tableWrap(
      'Documentos a conservar por obrigação, com a indicação de presença no cofre.',
      '<th scope="col" class="vc"><span class="sr-only">Presente no cofre</span></th><th scope="col">Obrigação</th>' +
        '<th scope="col">Documentos a conservar</th><th scope="col">Estado</th>',
      rows.join(''),
    )
  );
}

/* --------------------------------------------------------------------------
   10. Regras e fontes
   -------------------------------------------------------------------------- */

function renderRegras(model) {
  const rules = model.rules ?? { sources: [], obligations: [], todo: [] };
  const summary = model.pack?.summary ?? {};
  const problems = model.pack?.problems ?? [];
  const freshness = model.pack?.freshness ?? null;

  const sourceRows = (rules.sources ?? [])
    .map((source) => {
      const url = safeUrl(source.url);
      return (
        '<tr>' +
        `<td class="pt">${esc(source.id)}</td>` +
        `<td>${esc(source.authority)}` +
        (source.retrievedAt ? `<span class="sub">recolhida em ${ptDate(source.retrievedAt)}</span>` : '') +
        '</td>' +
        `<td><span class="badge${source.tier === 'official' ? ' local' : ''}">${source.tier === 'official' ? 'oficial' : 'secundária'}</span></td>` +
        `<td>${esc(source.title)}</td>` +
        `<td class="pt">${url === null ? esc(source.url) : `<a href="${url}" rel="noopener noreferrer">${esc(source.url)}</a>`}</td>` +
        '</tr>'
      );
    })
    .join('');

  const obligationRows = (rules.obligations ?? [])
    .map((rule) => {
      const verification = VERIFICATION[rule.verification] ?? VERIFICATION.unverified;
      const basis = (rule.legalBasis ?? []).map((item) => `<span class="mono">${esc(item)}</span>`).join(' · ');
      return (
        '<tr>' +
        `<td><span class="ob">${esc(rule.title)}</span><span class="ob-src">${esc(rule.id)}</span></td>` +
        `<td><span class="tipo">${esc(KIND_LABEL[rule.kind] ?? rule.kind)}</span></td>` +
        `<td><span class="${verification.cls}"><span aria-hidden="true">${verification.glyph}</span> ${esc(verification.label)}</span>` +
        (rule.verifyNote ? `<span class="ob-note">${esc(rule.verifyNote)}</span>` : '') +
        '</td>' +
        `<td>${basis === '' ? '<span class="mini">sem base legal registada</span>' : basis}</td>` +
        '</tr>'
      );
    })
    .join('');

  const sourcesTable =
    sourceRows === ''
      ? emptyState('Sem fontes', 'O pacote de regras não traz fontes citadas.')
      : tableWrap(
          'Fontes citadas pelo pacote de regras, com autoridade, nível e endereço.',
          '<th scope="col">Fonte</th><th scope="col">Autoridade</th><th scope="col">Nível</th><th scope="col">Título</th><th scope="col">Endereço</th>',
          sourceRows,
        );

  const obligationsTable =
    obligationRows === ''
      ? emptyState('Sem regras', 'O pacote de regras não traz obrigações.')
      : tableWrap(
          'Obrigações do pacote de regras, com tipo, estado de verificação e base legal.',
          '<th scope="col">Regra</th><th scope="col">Tipo</th><th scope="col">Verificação</th><th scope="col">Base legal</th>',
          obligationRows,
        );

  const problemNotes =
    problems.length === 0
      ? ''
      : '<div class="notes">' +
        problems
          .map(
            (problem) =>
              `<p><span class="g${problem.level === 'error' ? ' danger' : ''}" aria-hidden="true">!</span>` +
              `<span><span class="mono">${esc(problem.path)}</span> — ${esc(problem.message)}</span></p>`,
          )
          .join('') +
        '</div>';

  return (
    '<section class="block" id="regras" aria-label="Regras e fontes">' +
    sectionHead(
      'Regras e fontes',
      `Pacote <span class="mono">pt/${esc(model.meta.packVersion)}</span> · ` +
        `${summary.obligations ?? 0} regras (${summary.verified ?? 0} verificadas · ${summary.partial ?? 0} parciais · ` +
        `${(summary.unverified ?? 0) + (summary.stale ?? 0)} por verificar) · ${summary.sources ?? 0} fontes`,
    ) +
    (freshness === null
      ? ''
      : `<div class="disc${freshness.status === 'current' ? ' pos' : ''}"><span class="sev ${freshness.status === 'current' ? 'sev-info' : 'sev-media'}">` +
        `<span class="g" aria-hidden="true">${freshness.status === 'current' ? '●' : '▲'}</span> ${freshness.status === 'current' ? 'Atual' : 'A rever'}</span>` +
        `<div><p class="a-t">Estado do pacote de regras</p><p class="a-d">${esc(freshness.message)}</p></div></div>`) +
    '<p class="subhead mt-12">Fontes citadas</p>' +
    sourcesTable +
    '<p class="subhead mt-12">Obrigações declaradas</p>' +
    obligationsTable +
    (problemNotes === '' ? '' : '<p class="subhead mt-12">Problemas do pacote</p>' + problemNotes) +
    ((rules.todo ?? []).length === 0
      ? ''
      : '<p class="subhead mt-12">Por confirmar (lista do pacote)</p>' + notes(rules.todo)) +
    '<p class="tnote">Cada regra cita a sua fonte e diz até onde foi verificada. <span class="mono">verificada</span> significa que o prazo ' +
    'ou o valor foi confirmado na fonte citada; <span class="mono">parcial</span> significa que só parte foi confirmada. ' +
    'Um valor com verificação parcial não é um valor confirmado.</p>' +
    '</section>'
  );
}

/* --------------------------------------------------------------------------
   11. Atualizar regras (o assistente limitado)
   -------------------------------------------------------------------------- */

function unitValue(value, unit) {
  if (value === null || value === undefined) return '<span class="mini">— sem valor no pacote</span>';
  switch (unit) {
    case 'EUR_cents':
      return eur(value);
    case 'basis_points':
      return bpPercent(value);
    case 'hundredths_of_ias':
      return `${INT.format(value)} <span class="mini">centésimos do IAS</span>`;
    case 'months':
      return `${INT.format(value)} ${value === 1 ? 'mês' : 'meses'}`;
    default:
      return esc(String(value));
  }
}

function renderAssistente(model) {
  const update = model.update ?? { variables: [], variableCount: 0, requestCharacters: 0, hasKey: false, keySource: '', pending: null, pendingChanges: 0 };
  const summary = model.pack?.summary ?? {};
  const variables = update.variables ?? [];
  const pending = update.pending ?? null;

  const keyRow =
    update.hasKey === true
      ? '<div><dt>Chave de API</dt><dd><span class="keyfield"><span class="mono">chave presente</span>' +
        `<span class="badge local">${esc(update.keySource || 'origem não indicada')}</span></span>` +
        '<small>A chave fica fora do cofre e nunca é escrita nos registos nem enviada para o browser.</small></dd></div>'
      : '<div><dt>Chave de API</dt><dd><span class="state off"><span aria-hidden="true">⊘</span> Sem chave configurada</span>' +
        '<small>O envio está desativado. Para o ativar: <span class="cmd">vnfin ai-key set</span>.</small></dd></div>';

  const variableRows = variables
    .map(
      (variable) =>
        '<tr>' +
        `<td><span class="ob">${esc(variable.label)}</span><span class="ob-src">${esc(variable.path)}</span></td>` +
        `<td class="n">${unitValue(variable.currentValue, variable.unit)}</td>` +
        `<td class="pt">${esc(variable.unit)}</td>` +
        '</tr>',
    )
    .join('');

  const variablesTable =
    variables.length === 0
      ? emptyState('Sem variáveis', 'O pacote de regras não expõe variáveis atualizáveis.')
      : tableWrap(
          'Variáveis de lei pública que o assistente pode propor, com o valor atual no pacote e a unidade.',
          '<th scope="col">Variável</th><th scope="col" class="n">Valor atual no pacote</th><th scope="col">Unidade</th>',
          variableRows,
        );

  const previewBlock =
    state.preview === false
      ? ''
      : '<div class="redact mt-10">' +
        '<div class="redact-head"><h3>Pedido que será enviado</h3><span class="badge local">sem dados pessoais</span></div>' +
        '<p class="redact-note"><span class="g" aria-hidden="true">✓</span><span>' +
        `Jurisdição <span class="mono">PT</span> · ano <span class="mono">${esc(summary.year ?? model.meta.year)}</span> · ` +
        `<span class="mono">${esc(update.variableCount ?? variables.length)}</span> variáveis · cerca de <span class="mono">${esc(update.requestCharacters ?? 0)}</span> caracteres. ` +
        'Contém apenas: os nomes das variáveis, os seus valores atuais no pacote, as unidades e os endereços oficiais já citados pelo pacote ' +
        `(<span class="mono">${esc(summary.sources ?? 0)}</span> fontes). Não contém: perfil, NIF, faturas, clientes, rendimentos ou alertas.` +
        '</span></p>' +
        '<div class="redact-act">' +
        (update.hasKey === true
          ? '<button type="button" class="btn btn-primary" data-act="send-update">Enviar à DeepSeek</button>' +
            '<button type="button" class="btn" data-act="cancel-preview">Cancelar</button>' +
            '<span class="cta-note">Esta é a única chamada de rede da aplicação. A resposta é guardada como proposta e não altera o pacote até tu mandares aplicar.</span>'
          : '<button type="button" class="btn" disabled>Enviar à DeepSeek</button>' +
            '<button type="button" class="btn" data-act="cancel-preview">Cancelar</button>' +
            '<span class="cta-note">Envio desativado: não há chave configurada. Corre <span class="cmd">vnfin ai-key set</span>.</span>') +
        '</div>' +
        '</div>';

  const pendingBlock =
    pending === null
      ? '<div class="empty mt-10"><strong>Sem proposta pendente</strong>Não há nenhuma resposta por aplicar. O pacote de regras em uso é o que está no disco.</div>'
      : (() => {
          const rows = (pending.values ?? [])
            .map((value) => {
              const changed = value.value !== null && value.value !== value.currentValue;
              const url = safeUrl(value.sourceUrl);
              return (
                `<tr${changed ? ' class="changed"' : ''}>` +
                `<td><span class="ob">${esc(value.label)}</span><span class="ob-src">${esc(value.path)}</span></td>` +
                `<td class="n">${unitValue(value.currentValue, value.unit)}</td>` +
                `<td class="n">${value.value === null ? '<span class="mini">sem proposta</span>' : unitValue(value.value, value.unit)}` +
                (changed ? ' <span class="badge">alterado</span>' : '') +
                '</td>' +
                `<td class="pt">${url === null ? (value.sourceUrl ? esc(value.sourceUrl) : '<span class="mini">sem fonte indicada</span>') : `<a href="${url}" rel="noopener noreferrer">${esc(value.sourceUrl)}</a>`}</td>` +
                `<td class="small">${value.note ? esc(value.note) : '—'}</td>` +
                '</tr>'
              );
            })
            .join('');

          const problems =
            (pending.problems ?? []).length === 0
              ? ''
              : '<div class="notes">' +
                pending.problems
                  .map((problem) => `<p><span class="g danger" aria-hidden="true">!</span><span>${esc(problem)}</span></p>`)
                  .join('') +
                '</div>';

          return (
            '<div class="redact mt-10">' +
            '<div class="redact-head"><h3>Proposta por aplicar</h3>' +
            `<span class="badge est">${esc(update.pendingChanges ?? 0)} valores alterados</span></div>` +
            `<p class="redact-note"><span class="g" aria-hidden="true">●</span><span>Proposta recebida em <span class="mono">${esc(pending.proposedAt)}</span> · ` +
            `modelo <span class="mono">${esc(pending.model)}</span> · referência <span class="mono">${esc(pending.asOf)}</span> · ` +
            `${esc((pending.values ?? []).length)} valores respondidos. Nada foi aplicado ao pacote.</span></p>` +
            tableWrap(
              'Valores propostos pelo assistente, comparados com os valores atuais do pacote, com a fonte indicada e nota.',
              '<th scope="col">Variável</th><th scope="col" class="n">Atual</th><th scope="col" class="n">Proposto</th>' +
                '<th scope="col">Fonte indicada</th><th scope="col">Nota</th>',
              rows,
            ) +
            problems +
            '<div class="redact-act">' +
            '<button type="button" class="btn btn-primary" data-act="apply-update" data-verified="false">Aplicar (por confirmar)</button>' +
            '<button type="button" class="btn" data-act="apply-update" data-verified="true">Aplicar e marcar confirmado</button>' +
            '<button type="button" class="btn" data-act="discard-update">Descartar</button>' +
            '<span class="cta-note">Aplicar grava os valores como propostos pelo assistente. Passá-los a confirmados é um ato teu, depois de leres a fonte.</span>' +
            '</div>' +
            '<div class="disc"><span class="sev sev-media"><span class="g" aria-hidden="true">▲</span> Atenção</span><div>' +
            '<p class="a-t">Aplicar e marcar confirmado afirma que leste a fonte citada</p>' +
            '<p class="a-d">O assistente propõe; nunca verifica. Só escolhe esta opção depois de abrires o endereço indicado e confirmares o valor. ' +
            'Enquanto um valor ficar por confirmar, os alertas do painel voltam a apontá-lo.</p></div></div>' +
            '</div>'
          );
        })();

  const sendBlock =
    '<div class="redact mt-10">' +
    '<div class="redact-head"><h3>Passo a passo do envio</h3>' +
    `<span class="badge ${update.hasKey === true ? 'local' : 'miss'}">${update.hasKey === true ? 'chave presente' : 'sem chave'}</span></div>` +
    '<div class="kv">' +
    '<div><dt>1. Pré-visualizar</dt><dd>Revela exatamente o que o pedido contém: as variáveis, o ano, o tamanho e as fontes citadas. Não envia nada.' +
    (state.preview === true ? ' <span class="badge ver">pré-visualização aberta</span>' : '') + '</dd></div>' +
    '<div><dt>2. Enviar</dt><dd>Envia o pedido ao modelo <span class="mono">deepseek-chat</span> e guarda a resposta como proposta pendente. ' +
    'É a única chamada de rede da aplicação.</dd></div>' +
    '<div><dt>3. Aplicar</dt><dd>Decide se a proposta entra no pacote — e se fica por confirmar ou marcada como confirmada por uma pessoa.</dd></div>' +
    '</div>' +
    '<div class="redact-act">' +
    (state.preview === true
      ? '<button type="button" class="btn" data-act="cancel-preview">Fechar pré-visualização</button>'
      : update.hasKey === true
        ? '<button type="button" class="btn btn-primary" data-act="preview-update">Pré-visualizar pedido</button>'
        : '<button type="button" class="btn" disabled>Pré-visualizar pedido</button><span class="cta-note">Disponível quando existir chave: <span class="cmd">vnfin ai-key set</span>.</span>') +
    '</div>' +
    '</div>';

  return (
    '<section class="block" id="assistente" aria-label="Atualização das regras">' +
    sectionHead(
      'Atualizar regras',
      'A única função do assistente: propor os valores que a lei muda de ano para ano. Não conversa, não aconselha e não vê os teus dados.',
    ) +
    '<div class="ia">' +
    '<div class="ia-cfg">' +
    '<div class="redact-head"><h3>O que é enviado neste pedido</h3><span class="badge local">sem dados pessoais</span></div>' +
    '<p class="redact-note no-border-top"><span class="g" aria-hidden="true">✓</span><span>' +
    'O pedido é construído apenas a partir do <strong>pacote de regras</strong>: nomes de variáveis, os seus valores atuais e os endereços ' +
    'oficiais já citados por cada regra. Perfil, faturas, clientes e rendimentos <strong>não têm caminho de código</strong> para entrar neste pedido.' +
    '</span></p>' +
    '<div class="kv">' +
    `<div><dt>Variáveis</dt><dd><span class="mono">${esc(update.variableCount ?? variables.length)}</span> variáveis de lei pública <small>taxas, limites, coeficientes, IAS</small></dd></div>` +
    `<div><dt>Tamanho do pedido</dt><dd><span class="mono">${esc(update.requestCharacters ?? 0)} caracteres</span> <small>calculado a partir do pacote, sem qualquer dado do contribuinte</small></dd></div>` +
    keyRow +
    '<div><dt>Validação</dt><dd><span class="mono">unidade + ordem de grandeza</span><small>Uma taxa de IVA de 0,23% é um inteiro de pontos base válido — e é recusada.</small></dd></div>' +
    '</div>' +
    '<div class="ia-notes">' +
    '<p><span class="g lock" aria-hidden="true">✓</span><span>O pedido não inclui o perfil, as faturas, os clientes nem os rendimentos.</span></p>' +
    '<p><span class="g" aria-hidden="true">●</span><span>O assistente propõe valores; nunca verifica nem afirma que estão em vigor.</span></p>' +
    '<p><span class="g" aria-hidden="true">●</span><span>O pacote de regras só é alterado quando tu mandas aplicar, e fica registado como proposto até confirmares.</span></p>' +
    '</div>' +
    '</div>' +
    '<div>' +
    variablesTable +
    sendBlock +
    previewBlock +
    pendingBlock +
    '</div>' +
    '</div>' +
    '</section>'
  );
}

/* --------------------------------------------------------------------------
   Perfil: o formulário do primeiro arranque, e a edição depois
   --------------------------------------------------------------------------

   Na primeira utilização o cofre não tem perfil — e sem perfil não há
   enquadramento, agenda nem indicadores. Em vez de exigir um comando de terminal,
   o painel abre este formulário e grava o resultado no mesmo `profile.json` que
   a linha de comandos usaria. As listas de regimes vêm do modelo
   (`profileOptions`), pelo mesmo motivo por que as taxas de IVA vêm do pacote de
   regras: o painel não decide o que a AT aceita.
   -------------------------------------------------------------------------- */

function profileFormHtml(model) {
  const profile = model.profile;
  const activity = profile?.activity ?? {};
  const options = model.profileOptions ?? {};
  const editing = profile !== null && profile !== undefined;
  const today = isIsoDate(model.meta?.today) ? model.meta.today : '';

  const select = (name, list, selected) =>
    (list ?? [])
      .map(
        (item) =>
          `<option value="${esc(item.value)}"${item.value === selected ? ' selected' : ''}>${esc(item.label)}</option>`,
      )
      .join('');

  const lead = editing
    ? '<p class="prof-lead">Edita o enquadramento gravado no cofre local. As caixas de volume de negócios mostram o que está guardado; ' +
      'apagar uma caixa remove esse valor do perfil e a aplicação volta a dizer que não o pode avaliar.</p>'
    : '<p class="prof-lead">Ainda não existe perfil neste cofre. Estes são os dados da tua declaração de início de atividade: ' +
      'sem eles a aplicação não sabe que obrigações te pertencem e recusa-se a adivinhá-los.</p>' +
      '<p class="prof-lead">Ficam gravados apenas em <span class="mono">' +
      esc(model.meta.dataDir) +
      '</span>, neste computador. Nada é enviado para fora.</p>';

  const caeHint =
    (activity.cae ?? []).length > 0
      ? `${esc(activity.cae[0].code)} — ${esc(activity.cae[0].description)}`
      : `por omissão ${esc(options.defaultCae?.code ?? '')} — ${esc(options.defaultCae?.description ?? '')}`;

  return (
    lead +
    '<form data-form="profile-full" novalidate>' +
    '<div class="form-grid">' +
    '<div class="field"><label for="pf-nif">NIF</label>' +
    `<input id="pf-nif" name="nif" type="text" inputmode="numeric" maxlength="20" autocomplete="off" required value="${esc(profile?.nif ?? '')}">` +
    '<small>Nove dígitos. O dígito de controlo é validado antes de gravar.</small></div>' +
    '<div class="field"><label for="pf-name">Nome</label>' +
    `<input id="pf-name" name="name" type="text" maxlength="120" autocomplete="off" required value="${esc(profile?.name ?? '')}">` +
    '<small>Nome do contribuinte, como consta na declaração.</small></div>' +
    '<div class="field"><label for="pf-start">Início de atividade</label>' +
    `<input id="pf-start" name="startDate" type="date" value="${esc(isIsoDate(activity.startDate) ? activity.startDate : today)}">` +
    '<small>Data da abertura de atividade, como no Portal das Finanças.</small></div>' +
    '<div class="field"><label for="pf-cae">CAE principal</label>' +
    `<input id="pf-cae" name="caeCode" type="text" maxlength="5" inputmode="numeric" autocomplete="off" value="${esc(activity.cae?.[0]?.code ?? '')}" placeholder="62010">` +
    `<small>Vazio mantém: ${caeHint}.</small></div>` +
    '<div class="field"><label for="pf-iva">Regime de IVA</label>' +
    `<select id="pf-iva" name="ivaRegime" required>${select('ivaRegime', options.ivaRegimes, profile?.iva?.regime)}</select>` +
    '<small>Tal como está no Portal das Finanças. A aplicação não o adivinha.</small></div>' +
    '<div class="field"><label for="pf-irs">Regime de IRS</label>' +
    `<select id="pf-irs" name="irsRegime">${select('irsRegime', options.irsRegimes, profile?.irs?.regime)}</select>` +
    '<small>O coeficiente do regime simplificado vem do pacote de regras, não daqui.</small></div>' +
    '<div class="field"><label for="pf-prev">Volume de negócios do ano anterior (€)</label>' +
    `<input id="pf-prev" name="turnoverPreviousYearCents" type="text" inputmode="decimal" autocomplete="off" value="${esc(centsToInput(activity.turnoverPreviousYearCents))}" placeholder="12 400,00">` +
    '<small>Em território nacional, no ano civil anterior.</small></div>' +
    '<div class="field"><label for="pf-curr">Volume de negócios esperado para este ano (€)</label>' +
    `<input id="pf-curr" name="turnoverCurrentYearExpectedCents" type="text" inputmode="decimal" autocomplete="off" value="${esc(centsToInput(activity.turnoverCurrentYearExpectedCents))}" placeholder="48 600,00">` +
    '<small>A tua estimativa: é o que permite avisar a meio do ano.</small></div>' +
    '<div class="field full"><span class="lbl">Clientes fora de Portugal</span>' +
    `<label class="check" for="pf-eu"><input id="pf-eu" name="intraCommunityOperations" type="checkbox"${activity.intraCommunityOperations === true ? ' checked' : ''}> <span>Presto serviços a clientes da União Europeia (operações intracomunitárias)</span></label>` +
    `<label class="check" for="pf-ex"><input id="pf-ex" name="exports" type="checkbox"${activity.exports === true ? ' checked' : ''}> <span>Presto serviços a clientes fora da União Europeia (exportações)</span></label>` +
    `<label class="check" for="pf-ss"><input id="pf-ss" name="startupExemptionActive" type="checkbox"${profile?.ss?.startupExemptionActive === true ? ' checked' : ''}> <span>Estou no período de isenção de contribuições do primeiro ano (Segurança Social)</span></label>` +
    '</div>' +
    '</div>' +
    '<div class="form-actions">' +
    `<button type="submit" class="btn btn-primary">${editing ? 'Guardar alterações' : 'Criar perfil e entrar'}</button>` +
    '<span class="note">Gravado no cofre local. Nada é enviado para fora deste computador.</span>' +
    '</div>' +
    '<div data-role="form-message"></div>' +
    '</form>' +
    '<div class="prof-notes">' +
    '<p class="subhead">Carregar um perfil já existente</p>' +
    '<p class="tnote flush">Um <span class="mono">profile.json</span> de outro cofre (ou de uma cópia de segurança) pode ser carregado aqui, ' +
    'ou o perfil ativo gravado num ficheiro para levar para outro computador.</p>' +
    '<div class="prof-foot">' +
    '<label class="btn btn-sm" for="pf-file">Carregar perfil de ficheiro…</label>' +
    '<input id="pf-file" type="file" accept="application/json,.json" hidden>' +
    '<button type="button" class="btn btn-sm btn-tertiary" data-act="export-profile">Exportar o perfil atual</button>' +
    '</div>' +
    '</div>'
  );
}

function renderProfileModal(model) {
  const overlay = document.getElementById('profile-overlay');
  const body = document.getElementById('profile-modal-body');
  const title = document.getElementById('profile-modal-title');
  if (overlay === null || body === null) return;

  if (model === null || model === undefined) {
    overlay.hidden = true;
    return;
  }

  const editing = model.profile !== null && model.profile !== undefined;
  if (title !== null) title.textContent = editing ? 'Editar perfil do contribuinte' : 'Criar perfil do contribuinte';
  body.innerHTML = profileFormHtml(model);
  overlay.hidden = !state.profileOpen;
}

function openProfileModal() {
  state.profileOpen = true;
  renderProfileModal(state.model);
  const first = document.querySelector('#profile-modal-body input, #profile-modal-body select');
  if (first !== null) first.focus();
}

function closeProfileModal() {
  state.profileOpen = false;
  const overlay = document.getElementById('profile-overlay');
  if (overlay !== null) overlay.hidden = true;
}

/**
 * A submitted profile. Creating and updating are the same form on purpose: the
 * difference is which values the person is looking at, not two different ideas
 * of what a profile is. `replace` is sent only when the user confirmed it.
 */
async function submitProfileFull(form) {
  const nif = fieldValue(form, 'nif');
  const name = fieldValue(form, 'name');
  if (nif === '') {
    formMessage(form, 'err', 'O NIF é obrigatório: é o número de contribuinte da atividade.');
    return;
  }
  if (name === '') {
    formMessage(form, 'err', 'O nome é obrigatório.');
    return;
  }

  const ivaRegime = fieldValue(form, 'ivaRegime');
  if (ivaRegime === '') {
    formMessage(form, 'err', 'Escolhe o regime de IVA declarado no Portal das Finanças.');
    return;
  }

  const startDate = fieldValue(form, 'startDate');
  if (startDate !== '' && !isIsoDate(startDate)) {
    formMessage(form, 'err', 'A data de início de atividade tem de estar no formato AAAA-MM-DD.');
    return;
  }

  const previous = parseCentsField(fieldValue(form, 'turnoverPreviousYearCents'));
  if (previous === undefined) {
    formMessage(form, 'err', 'Volume de negócios do ano anterior: escreve um valor em euros, por exemplo 12 400,00 (ou deixa vazio).');
    return;
  }
  const expected = parseCentsField(fieldValue(form, 'turnoverCurrentYearExpectedCents'));
  if (expected === undefined) {
    formMessage(form, 'err', 'Previsão para este ano: escreve um valor em euros, por exemplo 48 600,00 (ou deixa vazio).');
    return;
  }

  const body = {
    nif,
    name,
    ivaRegime,
    turnoverPreviousYearCents: previous,
    turnoverCurrentYearExpectedCents: expected,
    intraCommunityOperations: fieldChecked(form, 'intraCommunityOperations'),
    exports: fieldChecked(form, 'exports'),
    startupExemptionActive: fieldChecked(form, 'startupExemptionActive'),
  };
  if (startDate !== '') body.startDate = startDate;
  const irsRegime = fieldValue(form, 'irsRegime');
  if (irsRegime !== '') body.irsRegime = irsRegime;

  // A CAE typed by the person running the application, with the description the
  // form already shows. Leaving it empty keeps the default the model named.
  const caeCode = fieldValue(form, 'caeCode');
  if (caeCode !== '') {
    const stored = state.model?.profile?.activity?.cae?.[0] ?? null;
    if (stored !== null && stored.code === caeCode) {
      body.cae = [{ code: stored.code, description: stored.description, role: 'principal' }];
    } else if (/^\d{5}$/.test(caeCode) && state.model?.profileOptions?.defaultCae?.code === caeCode) {
      body.cae = [
        {
          code: caeCode,
          description: state.model.profileOptions.defaultCae.description,
          role: 'principal',
        },
      ];
    } else if (!/^\d{5}$/.test(caeCode)) {
      formMessage(form, 'err', 'O CAE tem de ter cinco dígitos (por exemplo 62010).');
      return;
    } else {
      formMessage(
        form,
        'err',
        `O CAE ${caeCode} ainda não está descrito neste perfil. Deixa o campo vazio para manter o atual, ` +
          'ou carrega um perfil já completo a partir de um ficheiro.',
      );
      return;
    }
  }

  // Read before the request: `send` replaces the model with the answer, so after
  // it awaits there is no way to tell whether a profile was just created.
  const created = state.model?.profile === null || state.model?.profile === undefined;

  const data = await send(API.profile, body);
  const warnings = Array.isArray(data.warnings) ? data.warnings : [];
  const notes = warnings.length === 0 ? '' : ` ${warnings.join(' ')}`;
  toast(
    created
      ? `Perfil criado no cofre local.${notes}`
      : `Perfil atualizado no cofre local.${notes}`,
  );
  closeProfileModal();
}

async function exportProfile() {
  const response = await fetch(API.profileExport, {
    headers: { 'X-VNFIN-Token': state.token, Accept: 'application/json' },
    credentials: 'omit',
    cache: 'no-store',
  });
  const text = await response.text();
  let data = null;
  try {
    data = JSON.parse(text);
  } catch {
    data = null;
  }
  if (!response.ok || data === null || data.profile === undefined) {
    throw new ApiError(
      data !== null && typeof data.error === 'string' ? data.error : 'não foi possível exportar o perfil.',
    );
  }

  const blob = new Blob([`${JSON.stringify(data.profile, null, 2)}\n`], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = `vn-finance-profile-${String(data.profile.nif ?? 'perfil')}.json`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
  toast('Perfil exportado para um ficheiro neste computador.');
}

/** Um `profile.json` escolhido no disco -> o mesmo POST que o formulário usa. */
async function importProfileFile(file) {
  if (file === null || file === undefined) return;
  if (file.size > 256 * 1024) throw new ApiError('o ficheiro é demasiado grande para ser um perfil.');
  let parsed = null;
  try {
    parsed = JSON.parse(await file.text());
  } catch {
    throw new ApiError('o ficheiro não é JSON válido: escolhe um profile.json do vn-finance.');
  }

  const hasProfile = state.model?.profile !== null && state.model?.profile !== undefined;
  if (
    hasProfile &&
    !window.confirm('Já existe um perfil neste cofre. Substituir o perfil atual pelo do ficheiro?')
  ) {
    return;
  }

  const data = await send(API.profileImport, { profile: parsed, replace: hasProfile });
  const warnings = Array.isArray(data.warnings) ? data.warnings : [];
  toast(
    warnings.length === 0
      ? 'Perfil carregado do ficheiro.'
      : `Perfil carregado do ficheiro. ${warnings.join(' ')}`,
  );
  closeProfileModal();
}

/* --------------------------------------------------------------------------
   Acompanhamento: perfil ausente (onboarding) e chave de sessão ausente
   -------------------------------------------------------------------------- */

function renderOnboarding(model) {
  const problems = model.pack?.problems ?? [];
  return (
    '<section class="block" aria-label="Primeiros passos">' +
    sectionHead('Primeiros passos', 'O painel não inventa dados') +
    '<div class="onboard">' +
    '<h2>Ainda não existe perfil neste cofre</h2>' +
    '<p>O painel lê o perfil, os recibos e o pacote de regras do cofre local. Sem perfil não há enquadramento, não há agenda e não há indicadores — ' +
    'e é deliberado que assim seja: um painel que preenche estes valores por estimativa seria um painel em que não se pode confiar.</p>' +
    '<p>O formulário de perfil tem os dados da tua declaração de início de atividade: NIF, nome, regime de IVA e a data de abertura. ' +
    'Sem eles a aplicação não sabe que obrigações te pertencem, e não os adivinha.</p>' +
    '<ul>' +
    '<li>O cofre fica em <span class="mono">' + esc(model.meta.dataDir) + '</span> e nunca sai deste computador.</li>' +
    '<li>O volume de negócios do ano anterior nunca é assumido: sem ele, a aplicação não avalia a isenção do art. 53.º do CIVA e diz que não avalia.</li>' +
    '<li>Um perfil já existente pode ser carregado de um ficheiro <span class="mono">profile.json</span>, no mesmo formulário.</li>' +
    '<li>Depois do perfil criado, esta página passa a mostrar enquadramento, agenda fiscal, recibos, Segurança Social, IVA e IRS.</li>' +
    '</ul>' +
    '<div class="acts">' +
    '<button type="button" class="btn btn-primary" data-act="open-profile">Preencher o perfil</button>' +
    '<button type="button" class="btn" data-act="retry">Recarregar o modelo</button>' +
    '<a class="btn" href="#regras">Ver regras e fontes</a>' +
    '</div>' +
    (problems.length === 0
      ? ''
      : '<p class="subhead mt-14">Problemas do pacote de regras</p>' +
        notes(problems.map((problem) => `${problem.path} — ${problem.message}`), 'danger')) +
    '</div>' +
    '</section>'
  );
}

function renderNoToken() {
  const host = document.getElementById('painel');
  if (host === null) return;
  host.setAttribute('aria-busy', 'false');
  host.innerHTML =
    '<section class="block" aria-label="Sessão sem chave">' +
    sectionHead('Sessão sem chave', 'Nenhum pedido foi feito ao servidor') +
    '<div class="onboard">' +
    '<h2 class="b-t">Esta página não tem a chave de sessão</h2>' +
    '<p>O servidor local só aceita pedidos com a chave que imprime ao arrancar. Sem ela, o painel não faz nenhum pedido autenticado — ' +
    'por isso não há dados para mostrar.</p>' +
    '<p>Abre o endereço exato que o comando seguinte imprimiu no terminal (tem a forma <span class="mono">http://127.0.0.1:PORT/?t=…</span>):</p>' +
    '<p><span class="cmd">vnfin web</span></p>' +
    '<ul>' +
    '<li>A chave é guardada apenas na sessão deste browser e é retirada da barra de endereços.</li>' +
    '<li>Nada é enviado para fora deste computador.</li>' +
    '</ul>' +
    '<div class="acts">' +
    '<button type="button" class="btn btn-primary" data-act="retry">Já tenho a chave · tentar de novo</button>' +
    '</div>' +
    '</div>' +
    '</section>';
}

/* --------------------------------------------------------------------------
   Rodapé
   -------------------------------------------------------------------------- */

function renderFooter(model) {
  const guarantees = model.guarantees ?? [];
  const checksum = String(model.meta.packChecksum ?? '');
  return (
    '<footer class="foot">' +
    '<p class="rule">Regras pt/' +
    esc(model.meta.packVersion) +
    ' · verificação ' +
    esc(checksum.slice(0, 16)) +
    (checksum === '' ? '' : '…') +
    ' · fontes citadas em cada regra</p>' +
    (guarantees.length === 0
      ? ''
      : '<p class="subhead mt-8">O que esta aplicação não faz</p>' +
        '<ul>' +
        guarantees.map((item) => `<li>${esc(item)}</li>`).join('') +
        '</ul>') +
    '<p class="warn">Esta aplicação não substitui um contabilista certificado e não entrega declarações por ti.</p>' +
    '<p class="local">' +
    '<svg viewBox="0 0 16 16" aria-hidden="true"><rect x="3" y="7" width="10" height="7" rx="1.2"/><path d="M5.6 7V5.2a2.4 2.4 0 014.8 0V7"/></svg>' +
    '<span>Todos os dados residem neste computador · sem telemetria · sem servidores · painel local v' +
    esc(model.meta.version) +
    '</span>' +
    '</p>' +
    '</footer>'
  );
}

/* --------------------------------------------------------------------------
   Render principal
   -------------------------------------------------------------------------- */

function render(model) {
  renderChrome(model);
  const host = document.getElementById('painel');
  if (host === null) return;
  host.setAttribute('aria-busy', 'false');

  // First run: there is nothing to show until the profile exists, so the form is
  // what opens. Asked once per page, so closing it to read the rules does not
  // have it spring back on the next render.
  const hasProfile = model.profile !== null && model.profile !== undefined;
  if (!hasProfile && !state.profilePrompted) {
    state.profilePrompted = true;
    state.profileOpen = true;
  }
  renderProfileModal(model);

  const html = [renderAlertBlock(model)];
  if (model.profile === null || model.profile === undefined) {
    html.push(renderOnboarding(model));
  } else {
    html.push(renderKpis(model));
    html.push(renderEnquadramento(model));
    html.push(renderAgenda(model));
    html.push(renderRecibos(model));
    html.push(renderSs(model));
    html.push(renderIvaIrs(model));
    html.push(renderCofre(model));
    html.push(renderRegras(model));
  }
  html.push(renderAssistente(model));
  html.push(renderFooter(model));
  host.innerHTML = html.join('');
}

/* --------------------------------------------------------------------------
   Interação
   -------------------------------------------------------------------------- */

async function withBusy(button, work) {
  const label = button === null ? '' : button.textContent;
  if (button !== null) {
    button.disabled = true;
    button.setAttribute('aria-busy', 'true');
    button.textContent = 'A processar…';
  }
  try {
    return await work();
  } finally {
    if (button !== null && button.isConnected) {
      button.disabled = false;
      button.removeAttribute('aria-busy');
      button.textContent = label;
    }
  }
}

function formMessage(form, kind, message) {
  const host = form.querySelector('[data-role="form-message"]');
  if (host === null) return;
  host.innerHTML = `<p class="${kind === 'ok' ? 'form-ok' : 'form-error'}">${esc(message)}</p>`;
}

function fieldValue(form, name) {
  const field = form.elements.namedItem(name);
  if (field === null) return '';
  return typeof field.value === 'string' ? field.value.trim() : '';
}

function fieldChecked(form, name) {
  const field = form.elements.namedItem(name);
  return field !== null && field.checked === true;
}

/** Ledger -> pré-seleções do formulário. Só olha para o que já está registado. */
function computeLedgerDefaults(invoices) {
  const pick = (list) => {
    if (list.length === 0) return null;
    const count = (get) => {
      const map = new Map();
      for (const invoice of list) {
        const key = String(get(invoice));
        map.set(key, (map.get(key) ?? 0) + 1);
      }
      return [...map.entries()].sort((a, b) => b[1] - a[1])[0][0];
    };
    return {
      ivaBp: Number(count((invoice) => invoice.ivaRateBp)),
      retentionBp: Number(count((invoice) => invoice.retentionBp)),
      treatment: count((invoice) => invoice.vatTreatment),
    };
  };
  const pt = invoices.filter((invoice) => invoice.clientCountry === 'PT');
  const foreign = invoices.filter((invoice) => invoice.clientCountry !== 'PT');
  state.ledger = { pt: pick(pt), foreign: pick(foreign) };
}

function applyCountryDefaults() {
  const country = document.getElementById('f-country');
  const other = document.getElementById('f-country-other-wrap');
  const treatment = document.getElementById('f-treatment');
  const iva = document.getElementById('f-iva');
  const retention = document.getElementById('f-ret');
  if (country === null || treatment === null || iva === null || retention === null) return;

  const isOther = country.value === '__outro__';
  if (other !== null) other.hidden = !isOther;

  const group = isOther ? 'terceiros' : (country.selectedOptions[0]?.dataset.group ?? 'terceiros');
  // Defaults come from the rule pack (model.defaults) when there is no history to
  // learn from. Deriving them from previous invoices alone left the FIRST invoice
  // of a client group with empty rates, which the API then refused.
  const pack = state.model?.defaults ?? {};
  const bps = (value) => (typeof value === 'number' && Number.isFinite(value) ? value : null);
  if (group === 'pt') {
    const pt = state.ledger.pt;
    treatment.value = pt?.treatment ?? (state.model?.profile?.iva?.regime === 'isento_art53' ? 'isento_art53' : 'iva_pt');
    const fromHistory = pt === null || pt === undefined ? null : bps(pt.ivaBp);
    const fromPack = treatment.value === 'iva_pt' ? bps(pack.ivaNormalBp) : 0;
    iva.value = bpToInput(fromHistory ?? fromPack ?? 0);
    const retentionDefault = pt === null || pt === undefined ? null : bps(pt.retentionBp);
    retention.value = bpToInput(retentionDefault ?? bps(pack.withholdingResidentBp) ?? 0);
  } else {
    const foreign = state.ledger.foreign;
    treatment.value = foreign?.treatment ?? (group === 'ue' ? 'autoliquidacao_ue' : 'exportacao');
    iva.value = '0';
    const foreignDefault = foreign === null || foreign === undefined ? null : bps(foreign.retentionBp);
    retention.value = bpToInput(foreignDefault ?? 0);
  }
}

async function submitProfile(form) {
  const body = {};

  const previous = parseEurosToCents(fieldValue(form, 'turnoverPreviousYearCents'));
  if (previous === undefined) {
    formMessage(form, 'err', 'Volume de negócios do ano anterior: escreve um valor em euros, por exemplo 12 400,00.');
    return;
  }
  if (previous !== null) body.turnoverPreviousYearCents = previous;

  const current = parseEurosToCents(fieldValue(form, 'turnoverCurrentYearExpectedCents'));
  if (current === undefined) {
    formMessage(form, 'err', 'Previsão para o ano corrente: escreve um valor em euros, por exemplo 48 600,00.');
    return;
  }
  if (current !== null) body.turnoverCurrentYearExpectedCents = current;

  const regime = fieldValue(form, 'ivaRegime');
  if (regime !== '') body.ivaRegime = regime;
  body.intraCommunityOperations = fieldChecked(form, 'intraCommunityOperations');
  body.exports = fieldChecked(form, 'exports');

  await send(API.profile, body);
  toast('Enquadramento atualizado no cofre local.');
}

async function submitInvoice(form) {
  const body = {};

  const number = fieldValue(form, 'number');
  if (number !== '') body.number = number;

  const date = fieldValue(form, 'date');
  if (!isIsoDate(date)) {
    formMessage(form, 'err', 'Data de emissão: usa o formato AAAA-MM-DD do campo de data.');
    return;
  }
  body.date = date;

  const clientName = fieldValue(form, 'clientName');
  if (clientName === '') {
    formMessage(form, 'err', 'O nome do cliente é obrigatório.');
    return;
  }
  body.clientName = clientName;

  const clientNif = fieldValue(form, 'clientNif');
  if (clientNif !== '') body.clientNif = clientNif;

  let country = fieldValue(form, 'clientCountry');
  if (country === '__outro__') {
    const other = fieldValue(form, 'clientCountryOther').toUpperCase();
    if (!/^[A-Z]{2}$/.test(other)) {
      formMessage(form, 'err', 'Indica o código do país com duas letras (por exemplo US).');
      return;
    }
    country = other;
  }
  body.clientCountry = country;

  const description = fieldValue(form, 'description');
  if (description !== '') body.description = description;

  const base = parseEurosToCents(fieldValue(form, 'baseCents'));
  if (base === undefined || base === null || base <= 0) {
    formMessage(form, 'err', 'Base de incidência: escreve um valor em euros maior do que zero, por exemplo 1 200,50.');
    return;
  }
  body.baseCents = base;

  const iva = parsePercentToBp(fieldValue(form, 'ivaRateBp'));
  if (iva === undefined || iva === null || iva < 0) {
    formMessage(form, 'err', 'Taxa de IVA: escreve a percentagem, por exemplo 23 ou 0.');
    return;
  }
  body.ivaRateBp = iva;

  const treatment = fieldValue(form, 'vatTreatment');
  if (treatment !== '') body.vatTreatment = treatment;

  const retention = parsePercentToBp(fieldValue(form, 'retentionBp'));
  if (retention === undefined || retention < 0) {
    formMessage(form, 'err', 'Retenção na fonte: escreve a percentagem, por exemplo 25 ou 0.');
    return;
  }
  body.retentionBp = retention === null ? 0 : retention;

  const atcud = fieldValue(form, 'atcud');
  if (atcud !== '') body.atcud = atcud;

  const status = fieldValue(form, 'status');
  if (status !== '') body.status = status;

  body.paymentProofInVault = fieldChecked(form, 'paymentProofInVault');

  await send(API.invoices, body);
  toast('Recibo registado no cofre local.');
}

async function submitDocument(form) {
  const path = fieldValue(form, 'path');
  if (path === '') {
    formMessage(form, 'err', 'Indica o caminho absoluto do ficheiro neste computador.');
    return;
  }
  const body = { path };
  const kind = fieldValue(form, 'kind');
  if (kind !== '') body.kind = kind;
  const obligationId = fieldValue(form, 'obligationId');
  if (obligationId !== '') body.obligationId = obligationId;

  const data = await send(API.documents, body);
  const added = data.added ?? null;
  toast(
    added === null
      ? 'Documento registado no cofre.'
      : `Documento no cofre: ${added.file} · sha256 ${String(added.sha256 ?? '').slice(0, 16)}…`,
  );
}

document.addEventListener('click', (event) => {
  if (!(event.target instanceof Element)) return;
  const trigger = event.target.closest('[data-act]');
  if (trigger === null || trigger.disabled === true) return;
  const act = trigger.dataset.act;

  if (act === 'retry') {
    clearError();
    try {
      const stored = window.sessionStorage.getItem(TOKEN_KEY);
      if ((state.token === '' || state.token === null) && typeof stored === 'string') state.token = stored;
    } catch {
      /* sessão indisponível: mantém-se a chave que já existe nesta página. */
    }
    void boot({ silent: true });
    return;
  }
  if (act === 'open-profile') {
    openProfileModal();
    return;
  }
  if (act === 'close-profile') {
    closeProfileModal();
    return;
  }
  if (act === 'export-profile') {
    const host = document.getElementById('profile-modal-body');
    void withBusy(trigger, () => exportProfile()).catch((error) => {
      const message = error instanceof Error ? error.message : String(error);
      if (host !== null) formMessage(host, 'err', message);
      showError(message);
    });
    return;
  }
  if (act === 'dismiss-error') {
    clearError();
    return;
  }
  if (act === 'scroll-agenda') {
    document.getElementById('agenda')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
    return;
  }
  if (act === 'filter') {
    state.filter = trigger.dataset.filter ?? 'year';
    if (state.model !== null) applyModel(state.model);
    return;
  }
  if (act === 'open-invoice') {
    const details = document.getElementById('nova-fatura');
    if (details !== null) {
      details.open = true;
      details.scrollIntoView({ behavior: 'smooth', block: 'center' });
      const first = details.querySelector('input,select');
      if (first !== null) first.focus();
    }
    return;
  }
  if (act === 'preview-update') {
    state.preview = true;
    if (state.model !== null) applyModel(state.model);
    document.getElementById('assistente')?.scrollIntoView({ block: 'start' });
    return;
  }
  if (act === 'cancel-preview') {
    state.preview = false;
    if (state.model !== null) applyModel(state.model);
    return;
  }
  if (act === 'send-update') {
    void withBusy(trigger, async () => {
      await send(API.updateSend, {});
      state.preview = false;
      toast('Pedido enviado. A resposta ficou guardada como proposta pendente.');
    }).catch((error) => {
      showError(error instanceof Error ? error.message : String(error));
    });
    return;
  }
  if (act === 'apply-update') {
    const verified = trigger.dataset.verified === 'true';
    void withBusy(trigger, async () => {
      const data = await send(API.updateApply, { verified });
      const applied = typeof data.applied === 'number' ? data.applied : null;
      toast(
        verified
          ? `${applied === null ? 'Proposta' : applied} valores aplicados e marcados como confirmados.`
          : `${applied === null ? 'Proposta' : applied} valores aplicados ao pacote, por confirmar.`,
      );
    }).catch((error) => {
      showError(error instanceof Error ? error.message : String(error));
    });
    return;
  }
  if (act === 'discard-update') {
    void withBusy(trigger, async () => {
      await send(API.updateDiscard, {});
      toast('Proposta descartada. O pacote de regras não foi alterado.');
    }).catch((error) => {
      showError(error instanceof Error ? error.message : String(error));
    });
    return;
  }
  if (act === 'complete') {
    const body = {
      id: trigger.dataset.id ?? '',
      ruleId: trigger.dataset.rule ?? '',
      dueDate: trigger.dataset.due ?? '',
    };
    void withBusy(trigger, async () => {
      await send(API.complete, body);
      toast('Obrigação marcada como tratada.');
    }).catch((error) => {
      showError(error instanceof Error ? error.message : String(error));
    });
    return;
  }
  if (act === 'reveal-nif') {
    const nifNode = document.getElementById('nif-val');
    const reveal = document.getElementById('reveal-btn');
    const revealText = document.getElementById('reveal-txt');
    if (nifNode === null || reveal === null || revealText === null) return;
    const full = nifNode.dataset.full;
    if (full === undefined) return;
    state.nifRevealed = reveal.getAttribute('aria-pressed') !== 'true';
    nifNode.textContent = state.nifRevealed ? full : (nifNode.dataset.mask ?? '');
    nifNode.className = state.nifRevealed ? 'nif is-revealed' : 'nif';
    reveal.setAttribute('aria-pressed', state.nifRevealed ? 'true' : 'false');
    revealText.textContent = state.nifRevealed ? 'ocultar' : 'revelar';
  }
});

document.addEventListener('submit', (event) => {
  const form = event.target;
  if (!(form instanceof HTMLFormElement)) return;
  const kind = form.dataset.form;
  if (kind === undefined) return;
  event.preventDefault();
  formMessage(form, 'ok', '');
  const button = form.querySelector('button[type="submit"]');
  const work =
    kind === 'profile'
      ? () => submitProfile(form)
      : kind === 'profile-full'
        ? () => submitProfileFull(form)
        : kind === 'invoice'
          ? () => submitInvoice(form)
          : kind === 'document'
            ? () => submitDocument(form)
            : null;
  if (work === null) return;
  void withBusy(button, work).catch((error) => {
    formMessage(form, 'err', error instanceof Error ? error.message : String(error));
    showError(error instanceof Error ? error.message : String(error));
  });
});

document.addEventListener('change', (event) => {
  const target = event.target;
  if (target instanceof HTMLSelectElement && target.id === 'f-country') applyCountryDefaults();
  if (target instanceof HTMLInputElement && target.id === 'pf-file') {
    const file = target.files === null ? null : target.files[0];
    target.value = '';
    const host = document.getElementById('profile-modal-body');
    void withBusy(null, () => importProfileFile(file ?? null)).catch((error) => {
      const message = error instanceof Error ? error.message : String(error);
      if (host !== null) formMessage(host, 'err', message);
      showError(message);
    });
  }
});

/* O formulário de perfil é um modal: fechar com o rato no fundo escuro ou com a
   tecla Escape, como qualquer caixa de diálogo. O botão "Fechar" já existe. */
document.addEventListener('click', (event) => {
  if (!(event.target instanceof Element)) return;
  if (event.target.id === 'profile-overlay') closeProfileModal();
});

document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape' && state.profileOpen) closeProfileModal();
});

/* --------------------------------------------------------------------------
   Arranque
   -------------------------------------------------------------------------- */

async function boot({ silent = false } = {}) {
  if (!silent) {
    const host = document.getElementById('painel');
    if (host !== null) {
      host.setAttribute('aria-busy', 'true');
      host.innerHTML =
        '<p class="loading">A carregar o painel local…</p>' +
        '<span class="sk w40"></span><span class="sk w90"></span><span class="sk w70"></span><span class="sk w40"></span>';
    }
  }

  if (state.token === '') {
    renderNoToken();
    return;
  }

  try {
    const data = await request(API.dashboard, 'GET');
    const model = data !== null && typeof data === 'object' && data.model !== undefined ? data.model : data;
    if (model === null || typeof model !== 'object' || model.meta === undefined) {
      throw new ApiError('A resposta do servidor não tem a forma do modelo do painel.');
    }
    clearError();
    computeLedgerDefaults(model.invoices ?? []);
    applyModel(model);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const host = document.getElementById('painel');
    if (host !== null) {
      host.setAttribute('aria-busy', 'false');
      host.innerHTML =
        '<div class="onboard"><h2>Não foi possível ler o modelo local</h2>' +
        '<p>O servidor local não devolveu o painel. Os dados continuam no cofre, neste computador: nada foi perdido.</p>' +
        '<div class="acts"><button type="button" class="btn btn-primary" data-act="retry">Tentar de novo</button></div></div>';
    }
    showError(message);
  }
}

state.token = bootstrapToken();

void boot();
