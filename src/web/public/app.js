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
  complete: '/api/obligations/complete',
  documents: '/api/documents',
  documentsUpload: '/api/documents/upload',
  vault: '/api/vault',
  vaultBrowse: '/api/vault/browse',
  vaultReveal: '/api/vault/reveal',
  receiptsUpload: '/api/receipts/upload',
  receiptsRecord: '/api/receipts/record',
  aiKey: '/api/ai-key',
  aiKeyUnlock: '/api/ai-key/unlock',
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
  /* Que página está aberta, e a âncora dentro dela (quando houve uma). */
  page: 'painel',
  anchor: null,
  filter: 'year',
  preview: false,
  nifRevealed: false,
  /* O formulário de perfil abre sozinho na primeira utilização — quando o cofre
     ainda não tem perfil — e fica disponível no botão "Perfil" a partir daí. */
  profileOpen: false,
  profilePrompted: false,
  /* O escolhedor de pasta do cofre: a listagem vive no estado porque é uma
     resposta do servidor a uma navegação, e não faz parte do modelo fiscal. */
  vault: { open: false, listing: null, error: null, busy: false, asked: false },
  /* O resultado da leitura de um PDF: enquanto existir, a secção do cofre mostra
     o formulário de confirmação em vez de o esconder. Vive no estado porque a
     leitura é do servidor e o painel não a recalcula. */
  receipt: null,
};

/* --------------------------------------------------------------------------
   Páginas — os separadores do painel

   Cada separador é uma PÁGINA, não uma âncora numa página comprida: renderiza-se
   só o que pertence ao separador ativo. A razão é a de sempre neste projeto —
   quem abre isto pela primeira vez não sabe o que é uma "declaração periódica",
   e uma página comprida com dez secções todas abertas não ensina: obriga a
   procurar. Uma página por assunto, com um título e uma frase, ensina.

   A rota vive no `hash` do endereço (`#agenda`, `#cofre`, …), o que dá três
   coisas de graça: ligações diretas para um separador, o botão "voltar" do
   navegador a funcionar, e nenhuma alteração no servidor — o modelo continua a
   ser um só payload, e o painel decide o que mostrar dele.
   -------------------------------------------------------------------------- */

const PAGES = [
  {
    id: 'painel',
    label: 'Resumo',
    subtitle: 'Indicadores do exercício, enquadramento fiscal e alertas',
  },
  { id: 'agenda', label: 'Agenda fiscal', subtitle: 'O que a lei obriga, e quando' },
  { id: 'recibos', label: 'Faturas emitidas', subtitle: 'Os documentos que emitiste neste exercício' },
  { id: 'ss', label: 'Segurança Social', subtitle: 'A contribuição do trimestre, passo a passo' },
  { id: 'iva', label: 'IVA', subtitle: 'O IVA liquidado, por trimestre' },
  { id: 'irs', label: 'IRS', subtitle: 'Retenções sofridas, reserva e rendimento tributável' },
  {
    id: 'cofre',
    label: 'Cofre de documentos',
    subtitle: 'Onde vivem os ficheiros, o que está arquivado e as faturas em PDF',
  },
  { id: 'regras', label: 'Regras e fontes', subtitle: 'A lei citada por trás de cada número' },
  {
    id: 'diagnostico',
    label: 'Diagnóstico e chave',
    subtitle: 'Perfil, cofre, pacote de regras e chave da API',
  },
  { id: 'assistente', label: 'Atualizar regras', subtitle: 'A única função que usa a internet' },
  // A última de propósito: não é uma página de trabalho, é a promessa que
  // sustenta as outras todas. Estava em rodapé, em todos os separadores, onde
  // ninguém a lê duas vezes; aqui lê-se quando se quer saber o que a aplicação
  // não faz.
  { id: 'about', label: 'About', subtitle: 'O que esta aplicação não faz' },
];

/**
 * Páginas cujo conteúdo é sobre os teus números.
 *
 * Sem perfil não há enquadramento, não há agenda e não há estimativas — e o
 * painel prefere dizer isso a mostrar zeros que parecem dados. As outras páginas
 * (o cofre, as regras, o diagnóstico) funcionam sem perfil e continuam a
 * responder.
 */
const PROFILE_PAGES = new Set(['agenda', 'recibos', 'ss', 'iva', 'irs']);

/**
 * Âncoras que continuam a ser ligações válidas.
 *
 * "Porquê" numa linha da agenda aponta para o enquadramento, e o formulário de
 * fatura tem um id próprio: as duas coisas passam a significar "vai à página X e
 * desce até aqui", para que uma ligação escrita antes dos separadores continue a
 * levar ao mesmo sítio.
 */
const ANCHOR_PAGE = { enquadramento: 'painel' };

function pageById(id) {
  return PAGES.find((page) => page.id === id) ?? PAGES[0];
}

/** A rota pedida por um hash, já validada contra a lista de páginas. */
function routeFor(hash) {
  const raw = String(hash ?? '').replace(/^#\/?/, '');
  if (raw === '') return { page: 'painel', anchor: null };
  if (PAGES.some((page) => page.id === raw)) return { page: raw, anchor: null };
  const owner = ANCHOR_PAGE[raw];
  // Um hash desconhecido cai no Resumo, em vez de deixar um ecrã vazio.
  return owner === undefined ? { page: 'painel', anchor: null } : { page: owner, anchor: raw };
}

function currentRoute() {
  return routeFor(window.location.hash);
}

/**
 * Ir para uma página.
 *
 * Escreve o hash (é o que torna o separador uma ligação e faz o botão "voltar"
 * funcionar) e aplica a rota JÁ, sem esperar pelo evento `hashchange`: o evento
 * é assíncrono, e um clique que só produz efeito no instante seguinte parece
 * avariado. A duplicação é evitada em `applyRoute`, que compara com o que está
 * aplicado.
 */
function navigate(page, anchor = null) {
  const target = `${anchor === null ? page : anchor}`;
  if (window.location.hash !== `#${target}`) window.location.hash = `#${target}`;
  applyRoute({ page, anchor });
  if (anchor === null) window.scrollTo({ top: 0 });
}

function applyRoute(route) {
  const page = pageById(route.page);
  state.page = page.id;
  state.anchor = route.anchor;
  setActiveNav(page.id);
  if (state.model !== null) render(state.model);
  if (route.anchor !== null) {
    const target = document.getElementById(route.anchor);
    if (target !== null) target.scrollIntoView({ block: 'start' });
  }
}

/** Marca o separador ativo nas duas navegações: a barra lateral e a de baixo. */
function setActiveNav(id) {
  for (const link of document.querySelectorAll('[data-page]')) {
    const on = link.dataset.page === id;
    link.classList.toggle('on', on);
    if (on) link.setAttribute('aria-current', 'page');
    else link.removeAttribute('aria-current');
  }
}

/** A barra de separadores dos ecrãs estreitos, gerada da mesma lista. */
function renderTabbar() {
  const host = document.getElementById('tabbar');
  if (host === null || host.childElementCount === PAGES.length) return;
  host.innerHTML = PAGES.map(
    (page) =>
      `<a class="tab" href="#${esc(page.id)}" data-page="${esc(page.id)}" title="${esc(page.subtitle)}">${esc(page.label)}</a>`,
  ).join('');
}

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

/**
 * Um aviso não é um erro, e o painel não pode tratá-los como a mesma coisa.
 *
 * O mesmo sítio no ecrã serve os dois, mas com o peso visual certo: uma nota
 * diz "repara nisto" sem dizer "algo falhou". A distinção importa quando o
 * aviso é, por exemplo, a explicação que o próprio documento dá para não ter
 * havido retenção — isso não é uma falha de nada.
 */
function showNotice(message, kind = 'info') {
  const host = document.getElementById('aviso');
  if (host === null) return;
  host.innerHTML =
    `<div class="banner ${kind === 'warn' ? 'banner-warn' : 'banner-info'}">` +
    `<span class="sev ${kind === 'warn' ? 'sev-media' : 'sev-info'}"><span class="g" aria-hidden="true">${kind === 'warn' ? '●' : 'ℹ'}</span> ${kind === 'warn' ? 'Atenção' : 'Nota'}</span>` +
    `<div><p class="b-t">${kind === 'warn' ? 'Vale a pena confirmar' : 'Ficou registado assim'}</p>` +
    `<p class="b-d">${escLines(message)}</p></div>` +
    '<div class="b-a"><button type="button" class="btn btn-sm btn-tertiary" data-act="dismiss-error">Fechar</button></div>' +
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

/**
 * As regras que ainda não têm documento associado, uma por regra.
 *
 * Um documento responde por uma regra inteira: `documentedRules` é um conjunto de
 * ids de regra, e uma obrigação mensal é uma só regra por muitas vezes que caia no
 * ano. Contar as instâncias da agenda multiplicava o mesmo documento em falta por
 * doze e dizia à pessoa que tinha trinta e dois documentos para encontrar quando
 * estavam em causa onze regras — e o número caía cinco de uma vez ao arquivar um
 * único comprovativo, sem que nada explicasse porquê.
 */
function rulesMissingDocumentation(model) {
  const documented = documentedRules(model);
  const ids = new Set();
  for (const instance of model.agenda ?? []) {
    if (isActionable(instance) && needsDocumentation(instance) && !documented.has(instance.ruleId)) {
      ids.add(instance.ruleId);
    }
  }
  return ids;
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

/**
 * O cabeçalho de uma secção: título, o que se faz ali, e a frase que explica
 * porque é que a secção existe.
 *
 * `hint` é o dado do momento (quantos registos, que período); `lead` é a
 * explicação em português corrente. Separá-los é o que permite ler a página de
 * cima para baixo sem descodificar micro-etiquetas: primeiro o que é, depois o
 * número.
 */
function sectionHead(title, hint, lead) {
  return (
    '<div class="block-head">' +
    `<h2>${esc(title)}</h2>` +
    (hint ? `<span class="hint">${hint}</span>` : '') +
    '</div>' +
    (lead ? `<p class="lead">${lead}</p>` : '')
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
  const page = pageById(state.page);

  renderTabbar();
  setActiveNav(page.id);
  document.title = `vn-finance · ${page.label} · ${meta.year}`;
  setText('tb-sub', `${page.label} — ${page.subtitle}`);
  setText('chip-ano', String(meta.year));
  setText('chip-iva', profile === null ? 'IVA —' : `IVA ${ivaRegimeLabel(profile.iva?.regime)}`);
  setText('chip-irs', profile === null ? 'IRS —' : `IRS ${irsRegimeLabel(profile.irs?.regime)}`);
  setText('today-tag', `${ptDate(meta.today)} · hoje`);
  setText('print-line', `vn-finance · ${page.label} · exercício ${meta.year} · ${ptDate(meta.today)} · dados apenas neste computador`);

  const dir = document.getElementById('chip-dir');
  if (dir !== null) {
    dir.title = String(meta.dataDir ?? '');
    dir.innerHTML = `Cofre: <span class="mono">${esc(meta.dataDir)}</span>`;
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

  const missingDocs = rulesMissingDocumentation(model);
  setCount('cnt-cofre', String(missingDocs.size), missingDocs.size > 0 ? 'due' : 'off');

  setCount('cnt-regras', String(summary.obligations ?? 0), (model.pack?.problems ?? []).length > 0 ? 'soon' : '');

  /* O diagnóstico conta o que está por resolver: erros de perfil, cofre em git e
     pacote a rever. É o número que responde a "está tudo bem?". */
  const keyInfo = model.key ?? {};
  const profileErrors = (model.profileProblems ?? []).filter((problem) => problem.level === 'error').length;
  const risk = meta.dataDirRisk ?? { level: 'none' };
  const diagPoints =
    profileErrors + (risk.level === 'none' ? 0 : 1) + ((model.pack?.freshness ?? {}).status === 'current' ? 0 : 1);
  setCount('cnt-diag', diagPoints === 0 ? 'ok' : String(diagPoints), diagPoints === 0 ? 'off' : 'due');
  if (keyInfo.available !== true) setCount('cnt-diag', 'sem chave', diagPoints === 0 ? 'off' : 'due');

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

/**
 * A faixa de alerta — e a lista de alertas, que só pertence ao Resumo.
 *
 * Nas outras páginas aparece apenas quando há algo urgente. Repetir um "sem
 * alertas" verde em dez separadores tornaria o aviso em decoração, e decoração é
 * exactamente o que deixa de ser lida quando passa a importar.
 */
function renderAlertBlock(model, options = {}) {
  const onSummary = (options.page ?? 'painel') === 'painel';
  const today = model.meta.today;
  const flags = model.flags ?? [];
  const summary = model.flagSummary ?? { urgent: 0, attention: 0, info: 0, total: 0 };
  if (!onSummary && summary.urgent === 0) return '';
  const agenda = model.agenda ?? [];

  const next30 = agenda.filter((instance) => {
    if (!isActionable(instance) || instance.status === 'overdue') return false;
    const delta = daysBetween(today, instance.dueDate);
    return delta >= 0 && delta <= 30;
  });
  const overdue = agenda.filter((instance) => instance.status === 'overdue');
  const missingDocs = rulesMissingDocumentation(model);

  const top = flags.find((flag) => flag.severity === 'urgent') ?? flags[0] ?? null;
  const counts =
    `<span>${next30.length} ${next30.length === 1 ? 'obrigação' : 'obrigações'} nos próximos 30 dias</span>` +
    (overdue.length > 0 ? `<span class="dot" aria-hidden="true">·</span><span>${overdue.length} em atraso</span>` : '') +
    `<span class="dot" aria-hidden="true">·</span><span>${missingDocs.size} ${missingDocs.size === 1 ? 'regra sem documentação' : 'regras sem documentação'} no cofre</span>` +
    `<span class="dot" aria-hidden="true">·</span><span>${summary.urgent} urgentes · ${summary.attention} de atenção · ${summary.info} informativos</span>`;

  /*
   * As ações são construídas uma vez: a faixa aparece no Resumo e — quando há
   * algo urgente — nas outras páginas, e nas outras páginas leva também a ligação
   * para a lista completa, que só existe no Resumo.
   */
  const actions =
    '<div class="ab-actions">' +
    '<button type="button" class="btn btn-primary" data-act="scroll-agenda">Ver o plano</button>' +
    (onSummary
      ? ''
      : `<span class="ab-link">${summary.urgent} ${summary.urgent === 1 ? 'urgente' : 'urgentes'} · lista completa no <a href="#painel">Resumo</a></span>`) +
    '</div>';

  let strip;
  if (top === null) {
    strip =
      '<div class="alertbar pos">' +
      '<span class="pill p-ok"><span class="g" aria-hidden="true">✓</span>Sem alertas</span>' +
      '<div class="ab-body">' +
      '<p class="ab-title">Sem alertas fiscais ativos</p>' +
      `<p class="ab-meta">${counts}</p>` +
      '</div>' +
      actions +
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
      actions +
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

  // Nas outras páginas a faixa aponta para o Resumo, onde está a lista completa
  // com a base legal de cada alerta.
  if (!onSummary) {
    return `<section class="block" aria-label="Alertas urgentes">${strip}</section>`;
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
      'Um resumo do ano até hoje. Nenhum destes valores é uma previsão: todos saem das faturas que ' +
        'registaste e das constantes do pacote de regras, e cada um diz em que se baseia.',
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
      `<label class="check" for="p-eu50k"><input id="p-eu50k" name="intraCommunityOperationsAbove50k" type="checkbox"${activity.intraCommunityOperationsAbove50k === true ? ' checked' : ''}> <span>Essas operações passaram 50 000 EUR num trimestre (a declaração recapitulativa passa a mensal, mesmo no regime trimestral)</span></label>` +
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
      'É a tua situação declarada às Finanças, e é o que decide que obrigações aparecem na agenda. ' +
        'Corrigir aqui um valor muda a agenda; deixar um valor em branco faz a aplicação dizer que não ' +
        'consegue avaliar essa parte, em vez de a assumir.',
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
      'Cada linha é uma obrigação que a lei portuguesa impõe a quem trabalha por conta própria, com a data ' +
        'e a base legal de onde vem. A aplicação não entrega nem paga nada: serve para não haver surpresas.',
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

/*
 * A lista das faturas emitidas.
 *
 * Esta página MOSTRA o que entrou pelo Cofre: uma fatura nasce de um documento —
 * o PDF da fatura-recibo, arquivado com o seu hash e conferido linha a linha — e
 * não de um formulário preenchido à mão. Havia aqui um formulário "Nova
 * fatura-recibo" com quarenta campos de país, tratamento e taxas; era uma segunda
 * porta para a mesma sala, e a única porta que não deixava prova nenhuma do que
 * foi registado. Ficou uma porta só.
 */
function renderRecibos(model) {
  const meta = model.meta;
  const year = String(meta.year);
  const all = model.invoices ?? [];
  const invoices = all.filter((invoice) => String(invoice.date ?? '').startsWith(year));
  const others = all.length - invoices.length;

  const empty =
    invoices.length === 0
      ? '<div class="empty"><strong>Nada registado em ' +
        esc(year) +
        '</strong>' +
        (all.length === 0
          ? 'Ainda não há faturas neste cofre. Uma fatura entra aqui pelo documento que a prova: abre o ' +
            'Cofre de documentos, importa o PDF da fatura-recibo, confere o que foi lido e confirma.'
          : `Existem ${others} registos de outros exercícios no cofre, que não são apresentados aqui.`) +
        '<div class="acts mt-10"><a class="btn btn-primary" href="#cofre">Importar uma fatura em PDF</a></div>' +
        '</div>'
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
      '<td colspan="2">Derivado das taxas de cada documento</td>' +
      '</tr>';

    table = tableWrap(
      `Faturas e recibos emitidos em ${year}: número, data, cliente, base, IVA, retenção, líquido, estado e comprovativo.`,
      '<th scope="col">N.º</th><th scope="col">Data</th><th scope="col">Cliente</th>' +
        '<th scope="col" class="n">Base</th><th scope="col" class="n">IVA</th><th scope="col" class="n">Retenção</th>' +
        '<th scope="col" class="n">Líquido</th><th scope="col">Estado</th><th scope="col">Comprovativo</th>',
      rows + totalsRow,
    );
  }

  const footer =
    '<div class="tfoot">' +
    `<span>${invoices.length} ${invoices.length === 1 ? 'documento' : 'documentos'} de ${year}${others > 0 ? ` · ${others} registos de outros exercícios não apresentados` : ''}</span>` +
    '<span>O líquido é <span class="mono">base + IVA − retenção</span>, calculado a partir das taxas de cada documento.</span>' +
    '</div>';

  return (
    '<section class="block" id="recibos" aria-label="Faturas emitidas">' +
    sectionHead(
      `Faturas e recibos emitidos (${year})`,
      invoices.length === 0
        ? 'Sem faturação registada neste exercício'
        : `${invoices.length} ${invoices.length === 1 ? 'documento' : 'documentos'} · base ${eur(invoices.reduce((t, i) => t + i.baseCents, 0))}`,
      'As faturas que importaste no <a href="#cofre">Cofre de documentos</a> aparecem aqui. É desta lista ' +
        'que saem quase todos os números do painel: o IVA a entregar, a contribuição para a Segurança ' +
        'Social e o rendimento tributável do IRS. Cada linha traz o documento que a prova.',
    ) +
    empty +
    table +
    footer +
    '</section>'
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
      'Contribuição do trimestre, passo a passo, a partir da faturação registada',
      'A contribuição não incide sobre tudo o que faturas: incide sobre uma percentagem do rendimento de ' +
        'serviços, e é paga em três meses. Cada passo do cálculo aparece com o valor que usou, para se ' +
        'poder conferir em vez de acreditar.',
    ) +
    `<div class="ss"><div>${chain}${instalmentBlock}</div>${side}</div>` +
    (quarter.totals?.invoiceCount === 0
      ? '<p class="tnote">Não há recibos com data neste trimestre: a contribuição apresentada é zero por ausência de faturação registada.</p>'
      : '') +
    '</section>'
  );
}

/* --------------------------------------------------------------------------
   8. IVA e IRS — duas páginas, um só modelo

   Estavam juntos numa secção com uma tabela de sete colunas, e era essa a
   confusão: o IVA é dinheiro que ENTREGAS ao Estado todos os trimestres, a
   retenção é IRS que os clientes já entregaram POR TI, e o rendimento tributável
   é a base do IRS do ano seguinte. São três perguntas diferentes, e cada uma
   merece a sua página. Nada disto calcula nada de novo: as duas páginas leem os
   mesmos relatórios trimestrais do modelo.
   -------------------------------------------------------------------------- */

function quarterTotals(quarters) {
  const totals = { base: 0, iva: 0, retention: 0, invoices: 0, ss: 0 };
  for (const quarter of quarters) {
    totals.base += quarter.totals?.baseCents ?? 0;
    totals.iva += quarter.totals?.ivaLiquidadoCents ?? 0;
    totals.retention += quarter.totals?.retencaoSofridaCents ?? 0;
    totals.invoices += quarter.totals?.invoiceCount ?? 0;
    totals.ss += quarter.socialSecurity?.contributionCents ?? 0;
  }
  return totals;
}

function renderIva(model) {
  const quarters = model.quarters ?? [];
  if (quarters.length === 0) {
    return (
      '<section class="block" id="iva" aria-label="IVA">' +
      sectionHead('IVA', 'IVA liquidado por trimestre') +
      emptyState('Nada registado', 'O modelo não incluiu relatórios trimestrais.') +
      '</section>'
    );
  }

  const totals = quarterTotals(quarters);
  const rows = quarters
    .map(
      (quarter) =>
        '<tr>' +
        `<td>${esc(quarter.quarter)}.º trimestre</td>` +
        `<td class="d">${ptDate(quarter.start)} – ${ptDate(quarter.end)}</td>` +
        `<td class="n">${esc(quarter.totals?.invoiceCount ?? 0)}</td>` +
        `<td class="n">${eur(quarter.totals?.baseCents ?? 0)}</td>` +
        `<td class="n">${eur(quarter.totals?.ivaLiquidadoCents ?? 0)}</td>` +
        '</tr>',
    )
    .join('');

  const totalRow =
    '<tr class="tot">' +
    '<td colspan="2">Total do exercício</td>' +
    `<td class="n">${totals.invoices}</td>` +
    `<td class="n">${eur(totals.base)}</td>` +
    `<td class="n">${eur(totals.iva)}</td>` +
    '</tr>';

  return (
    '<section class="block" id="iva" aria-label="IVA">' +
    sectionHead(
      'IVA',
      `IVA liquidado em ${model.meta.year} · ${eur(totals.iva)}`,
      'O IVA é a parte da fatura que não é tua: recebes do cliente e entregas ao Estado. ' +
        'O valor é a soma do IVA das faturas que registaste em cada trimestre, e o prazo de entrega está ' +
        'na Agenda fiscal.',
    ) +
    tableWrap(
      'IVA liquidado por trimestre: documentos, base tributável e IVA.',
      '<th scope="col">Trimestre</th><th scope="col">Período</th><th scope="col" class="n">Documentos</th>' +
        '<th scope="col" class="n">Base tributável</th><th scope="col" class="n">IVA liquidado</th>',
      rows + totalRow,
    ) +
    '<p class="tnote">Sem registo de faturas não há IVA a entregar — e é por isso que esta página aparece a ' +
    'zero numa instalação nova. A declaração periódica entrega-se mesmo com zero, e o prazo está na agenda. ' +
    'Se estás isento pelo art. 53.º do CIVA, não liquidas IVA nenhum: esta página fica sem valores a entregar.</p>' +
    '</section>'
  );
}

function renderIrs(model) {
  const quarters = model.quarters ?? [];
  const reserve = model.reserve ?? { lowCents: 0, highCents: 0, basis: [], limitations: [] };

  if (quarters.length === 0) {
    return (
      '<section class="block" id="irs" aria-label="IRS">' +
      sectionHead('IRS', 'Rendimento tributável do exercício') +
      emptyState('Nada registado', 'O modelo não incluiu relatórios trimestrais.') +
      '</section>'
    );
  }

  const totals = quarterTotals(quarters);
  const rows = quarters
    .map(
      (quarter) =>
        '<tr>' +
        `<td>${esc(quarter.quarter)}.º trimestre</td>` +
        `<td class="d">${ptDate(quarter.start)} – ${ptDate(quarter.end)}</td>` +
        `<td class="n">${eur(quarter.totals?.baseCents ?? 0)}</td>` +
        `<td class="n">${eur(quarter.totals?.retencaoSofridaCents ?? 0)}</td>` +
        '</tr>',
    )
    .join('');

  const totalRow =
    '<tr class="tot">' +
    '<td colspan="2">Total do exercício</td>' +
    `<td class="n">${eur(totals.base)}</td>` +
    `<td class="n">${eur(totals.retention)}</td>` +
    '</tr>';

  return (
    '<section class="block" id="irs" aria-label="IRS">' +
    sectionHead(
      'IRS',
      `Rendimento de ${model.meta.year} · ${eur(totals.retention)} já retidos na fonte`,
      'A retenção na fonte não é um imposto a mais: é IRS que o teu cliente entregou ao Estado em teu nome, ' +
        'e que é creditado no acerto final. O rendimento tributável é a base sobre a qual esse acerto vai ' +
        'incidir — não é o imposto a pagar, porque as taxas do art. 68.º do CIRS não são aplicadas aqui.',
    ) +
    tableWrap(
      'Base tributável e retenção na fonte, por trimestre.',
      '<th scope="col">Trimestre</th><th scope="col">Período</th>' +
        '<th scope="col" class="n">Rendimento</th><th scope="col" class="n">Retenção sofrida</th>',
      rows + totalRow,
    ) +
    `<div class="panel mt-12 panel-pad">` +
    '<p class="subhead">Reserva recomendada</p>' +
    `<p class="dl-inline"><span class="kpi-v sm">${eur(reserve.lowCents)} – ${eur(reserve.highCents)}</span><span class="badge est">estimativa</span></p>` +
    '<p class="tnote">Quanto deste rendimento convém manter de lado para o IRS e a Segurança Social do próximo ' +
    'ano. É um intervalo, e não um valor: a reserva é uma decisão de tesouraria, não um cálculo fiscal.</p>' +
    ((reserve.basis ?? []).length === 0
      ? ''
      : `<p class="tnote">Base: ${(reserve.basis ?? []).map((item) => esc(item)).join(' · ')}</p>`) +
    notes(reserve.limitations) +
    '</div>' +
    '<p class="tnote">Esta página mostra as retenções que já te foram feitas e quanto convém reservar. O ' +
    'apuramento do rendimento tributável a partir das despesas documentadas — que exige decidir o que é ' +
    'elegível, um juízo e não um cálculo — não está aqui: as despesas ainda não fazem parte do modelo. ' +
    'Não há, em lado nenhum deste painel, um valor final de IRS a pagar, porque o motor para no rendimento ' +
    'tributável e as taxas do art. 68.º do CIRS não são aplicadas. Qualquer apuramento final exige as taxas ' +
    'do ano e as deduções, e é matéria de um contabilista certificado.</p>' +
    '</section>'
  );
}

/*
 * O apuramento do rendimento tributável a partir das despesas documentadas saiu
 * do painel: as despesas ainda não fazem parte do modelo — não há onde as
 * registar, nem onde as guardar — e uma caixa de texto que pede um número que
 * nada no cofre conhece é um formulário a mais, não uma capacidade a mais.
 * A conta continua no núcleo (`estimateIrsSimplifiedBase`) e na linha de
 * comandos (`vnfin estimate --despesas`); volta ao painel quando as despesas
 * forem um registo como as faturas são, e não um valor escrito de cada vez.
 */

/* --------------------------------------------------------------------------
   9. Cofre de documentos
   -------------------------------------------------------------------------- */

/* --------------------------------------------------------------------------
   Onde vive o cofre.

   O cofre é uma pasta: o perfil, os recibos, os documentos e o registo de
   auditoria vivem lá dentro, e mais nada. Duas coisas dependem de a pessoa saber
   isto — onde estão os dados, e como os copiar — por isso a pasta é mostrada,
   pode ser aberta no gestor de ficheiros e pode ser mudada a partir daqui. Uma
   pasta que ninguém escolheu e ninguém vê não é um cofre: é um sítio onde os
   dados calharam de ficar.
   -------------------------------------------------------------------------- */

/** true quando ainda ninguém escolheu a pasta e ela não tem nada dentro. */
function needsVaultChoice(model) {
  const meta = model.meta ?? {};
  return meta.dataDirSource === 'default' && meta.vaultInUse !== true && !state.vault.asked;
}

function renderVaultSetup(model) {
  const path = String(model.meta?.dataDir ?? '');
  return (
    '<section class="block" id="cofre-setup" aria-label="Escolher a pasta do cofre">' +
    sectionHead('Primeiro: onde deve viver o cofre', 'Nada foi escrito ainda') +
    '<div class="onboard">' +
    '<h2>Escolhe uma pasta deste computador para guardar os teus dados</h2>' +
    '<p>O <strong>cofre</strong> é uma pasta tua. Lá dentro ficam o perfil, as faturas registadas, ' +
    'os PDF e comprovativos que arquivares, e um registo de tudo o que a aplicação fez. ' +
    'Nada é enviado para a Internet e não há nenhuma cópia noutro sítio: fazer uma copia de segurança ' +
    'é copiar esta pasta.</p>' +
    '<p>Sem escolheres, a aplicação usaria <span class="mono">' + esc(path) + '</span> — uma pasta ' +
    'escondida na tua pasta pessoal, que é o género de sítio onde os dados ficam e ninguém os encontra. ' +
    'Uma pasta no Ambiente de Trabalho ou em Documentos é mais fácil de ver, de copiar e de guardar.</p>' +
    '<div class="acts">' +
    '<button type="button" class="btn btn-primary" data-act="open-vault-picker">Escolher a pasta do cofre…</button>' +
    '<button type="button" class="btn" data-act="use-default-vault">Usar a pasta por omissão</button>' +
    '</div>' +
    '<p class="sub mt-12">Podes mudar de pasta mais tarde, em <strong>Cofre de documentos</strong>. ' +
    'Mudar de pasta não apaga nada: o painel passa a mostrar a pasta nova e a antiga fica como está.</p>' +
    '</div>' +
    '</section>'
  );
}

/**
 * O escolhedor de pastas.
 *
 * É uma navegação de pastas dentro do próprio painel, e não a janela do sistema,
 * por uma razão concreta: uma página não recebe o caminho de uma pasta — só o seu
 * conteúdo — e o servidor precisa do caminho para escrever lá. Como o servidor é
 * local, é ele que lista as pastas; o painel só mostra o que ele devolve. Só
 * pastas, nunca ficheiros.
 */
/**
 * O caminho em migalhas clicáveis, com o separador que o caminho usa.
 *
 * Windows e POSIX separam pastas de forma diferente e um caminho pode começar
 * por `C:\`, por `/` ou por uma pasta relativa: juntar segmentos com uma barra
 * fixa produzia caminhos que não existem no outro sistema, e um caminho que não
 * existe é uma lista vazia e um botão que não faz nada.
 */
function pathCrumbs(path) {
  const raw = String(path ?? '');
  const separator = raw.includes('\\') ? '\\' : '/';
  const segments = raw.split(/[\\/]/).filter((part) => part !== '');
  const crumbs = [];
  let walked = '';

  if (/^[A-Za-z]:$/.test(segments[0] ?? '')) {
    walked = `${segments.shift()}\\`;
    crumbs.push({ label: walked, path: walked });
  } else if (raw.startsWith('/')) {
    walked = '/';
    crumbs.push({ label: 'raiz', path: '/' });
  }
  for (const segment of segments) {
    walked = walked === '' ? segment : walked.endsWith(separator) ? walked + segment : `${walked}${separator}${segment}`;
    crumbs.push({ label: segment, path: walked });
  }
  return crumbs;
}

/** Junta um nome a uma pasta com o separador certo. */
function joinPath(base, name) {
  const path = String(base ?? '');
  const separator = path.includes('\\') ? '\\' : '/';
  if (path === '') return name;
  return path.endsWith(separator) ? `${path}${name}` : `${path}${separator}${name}`;
}

function renderVaultPicker() {
  const listing = state.vault.listing;
  const host = document.getElementById('vault-picker-body');
  if (host === null) return;

  if (listing === null) {
    host.innerHTML = state.vault.error === null
      ? '<p class="loading">A ler as pastas…</p>'
      : `<div class="banner banner-err"><span class="b-t">Não foi possível ler as pastas</span><span class="b-d">${esc(state.vault.error)}</span></div>`;
    return;
  }

  const crumbs = pathCrumbs(listing.path)
    .map(
      (crumb, index) =>
        (index === 0 ? '' : '<span class="crumb-sep" aria-hidden="true">›</span>') +
        `<button type="button" class="crumb" data-act="vault-go" data-path="${esc(crumb.path)}">${esc(crumb.label)}</button>`,
    )
    .join('');

  const roots = (listing.roots ?? [])
    .map(
      (root) =>
        `<button type="button" class="chip-p" data-act="vault-go" data-path="${esc(root.path)}">${esc(root.label)}</button>`,
    )
    .join('');

  const entries = (listing.entries ?? [])
    .map(
      (entry) =>
        '<button type="button" class="vp-entry" data-act="vault-go" data-path="' + esc(entry.path) + '">' +
        '<span class="vp-ico" aria-hidden="true">📁</span>' +
        `<span class="vp-name">${esc(entry.name)}</span>` +
        (entry.isVault ? '<span class="badge local">já tem dados</span>' : '') +
        '</button>',
    )
    .join('');

  const risk = listing.risk ?? { level: 'none', message: null };
  const riskBlock =
    risk.level === 'none'
      ? ''
      : `<div class="banner ${risk.level === 'fatal' ? 'banner-err' : 'banner-info'}">` +
        `<span class="b-t">${risk.level === 'fatal' ? 'Esta pasta não pode ser o cofre' : 'Atenção a esta pasta'}</span>` +
        `<span class="b-d">${escLines(risk.message ?? '')}</span></div>`;

  const current = String(listing.path);
  const canUse = risk.level !== 'fatal';
  const already = state.model !== null && state.model.meta.dataDir === current;

  host.innerHTML =
    riskBlock +
    '<div class="vp-roots" role="group" aria-label="Atalhos">' + roots + '</div>' +
    '<nav class="vp-crumbs" aria-label="Caminho">' + crumbs + '</nav>' +
    (listing.error === null
      ? entries === ''
        ? '<p class="empty">Esta pasta não tem subpastas. Podes usá-la tal como está, ou criar uma pasta nova aqui.</p>'
        : '<div class="vp-list" role="list">' + entries + '</div>'
      : `<p class="empty"><strong>${esc(listing.error)}</strong>Podes criar a pasta aqui em baixo.</p>`) +
    '<div class="vp-new">' +
    '<label class="sr-only" for="vp-new-name">Nome da pasta nova</label>' +
    '<input id="vp-new-name" type="text" autocomplete="off" spellcheck="false" placeholder="nome da pasta nova">' +
    '<button type="button" class="btn" data-act="vault-new-folder">Criar e usar esta pasta</button>' +
    '</div>' +
    '<div class="vp-actions">' +
    '<span class="vp-target">Cofre em <span class="mono">' + esc(current) + '</span></span>' +
    `<button type="button" class="btn btn-primary" data-act="vault-use" data-path="${esc(current)}"${canUse ? '' : ' disabled'}>` +
    (already ? 'Usar esta pasta (a atual)' : 'Criar o cofre nesta pasta') +
    '</button>' +
    '</div>';
}

async function openVaultPicker(path) {
  state.vault.open = true;
  const overlay = document.getElementById('vault-overlay');
  if (overlay !== null) overlay.hidden = false;
  await loadVaultListing(path);
}

function closeVaultPicker() {
  state.vault.open = false;
  const overlay = document.getElementById('vault-overlay');
  if (overlay !== null) overlay.hidden = true;
}

async function loadVaultListing(path) {
  state.vault.error = null;
  renderVaultPicker();
  const query = path === null || path === undefined || path === '' ? '' : `?path=${encodeURIComponent(path)}`;
  try {
    const data = await request(`${API.vaultBrowse}${query}`, 'GET');
    state.vault.listing = data;
  } catch (error) {
    state.vault.listing = null;
    state.vault.error = error instanceof Error ? error.message : String(error);
  }
  renderVaultPicker();
}

/** Escolher (ou criar) a pasta do cofre, e passar a usá-la. */
async function chooseVault(path) {
  if (path === null || path === undefined || String(path).trim() === '') {
    showError('Escolhe uma pasta para o cofre.');
    return;
  }
  const data = await send(API.vault, { path: String(path) });
  state.vault.asked = true;
  // Uma leitura de PDF pertence ao cofre onde o ficheiro foi arquivado: mudar de
  // pasta sem a descartar deixaria um formulário a apontar para um documento que
  // já não está no cofre em uso.
  state.receipt = null;
  closeVaultPicker();
  const notes = Array.isArray(data.notes) ? data.notes : [];
  toast(
    notes.length === 0
      ? `Cofre: ${String(data.vault ?? path)}`
      : `Cofre: ${String(data.vault ?? path)} — ${notes[0]}`,
  );
  if (notes.length > 1) for (const note of notes.slice(1)) showError(note);
}

async function revealVault(button) {
  await withBusy(button, () => send(API.vaultReveal, {}));
  toast('Pasta do cofre aberta no gestor de ficheiros.');
}

function renderCofre(model) {
  const vault = model.vault ?? { entries: [], files: 0, bytes: 0 };
  const entries = vault.entries ?? [];
  const documented = documentedRules(model);
  const missing = rulesMissingDocumentation(model);

  const head =
    '<div class="vault-head">' +
    (missing.size > 0
      ? `<span class="pill p-danger"><span class="g" aria-hidden="true">▲</span>${missing.size} ${missing.size === 1 ? 'regra em falta' : 'regras em falta'}</span>`
      : '<span class="pill p-ok"><span class="g" aria-hidden="true">✓</span>Sem pendências conhecidas</span>') +
    '<span class="vault-sum">' +
    `<span>${esc(vault.files)} ${vault.files === 1 ? 'ficheiro' : 'ficheiros'} no cofre · ${bytesLabel(vault.bytes)}</span>` +
    `<span>Índice: ${entries.length} ${entries.length === 1 ? 'entrada' : 'entradas'}</span>` +
    '</span>' +
    '</div>';

  const { upload, form, table } = vaultArchiveForms(entries, model);

  return (
    '<section class="block" id="cofre" aria-label="Cofre de documentos">' +
    sectionHead(
      'Cofre de documentos',
      `${entries.length} ${entries.length === 1 ? 'documento indexado' : 'documentos indexados'}`,
      'Tudo o que arquivares fica nesta pasta, identificado pelo hash do próprio ficheiro, para se poder ' +
        'provar mais tarde que não mudou. O original nunca é alterado: é feita uma cópia.',
    ) +
    vaultLocationCard(model) +
    head +
    receiptImport() +
    (state.receipt === null ? '' : receiptReview(state.receipt)) +
    upload +
    form +
    table +
    checklistTable(model, documented) +
    '<p class="tnote">O cofre é uma pasta local indexada com o hash de cada ficheiro. Nada é copiado para a nuvem e o painel nunca envia o conteúdo dos documentos para lado nenhum.</p>' +
    '</section>'
  );
}

/** O cartão que responde a "onde é que isto está?". */
function vaultLocationCard(model) {
  const meta = model.meta ?? {};
  const source = String(meta.dataDirSource ?? 'flag');
  const origin =
    source === 'pointer'
      ? 'Pasta escolhida por ti e lembrada para as próximas vezes.'
      : source === 'env'
        ? 'Pasta indicada pela variável de ambiente VN_FINANCE_DATA_DIR.'
        : source === 'flag'
          ? 'Pasta indicada na linha de comandos (--vault ou --data-dir).'
          : 'Pasta por omissão da aplicação, ainda não escolhida por ti.';
  const risk = meta.dataDirRisk ?? { level: 'none', message: null };
  return (
    '<div class="vault-loc">' +
    '<div class="vl-main">' +
    '<span class="vl-k">Pasta do cofre</span>' +
    `<span class="vl-path mono">${esc(meta.dataDir)}</span>` +
    `<span class="vl-src">${esc(origin)}</span>` +
    '</div>' +
    '<div class="vl-acts">' +
    '<button type="button" class="btn btn-sm" data-act="reveal-vault" title="Abrir esta pasta no Explorador de Ficheiros">Abrir a pasta</button>' +
    '<button type="button" class="btn btn-sm" data-act="open-vault-picker">Mudar de pasta…</button>' +
    '</div>' +
    (risk.level === 'none'
      ? ''
      : `<div class="vl-risk ${risk.level === 'fatal' ? 'bad' : 'warn'}">${escLines(risk.message ?? '')}</div>`) +
    '</div>'
  );
}

/** Ler uma fatura-recibo em PDF: o primeiro passo, antes de confirmar. */
function receiptImport() {
  return (
    '<details class="disclosure noprint" open>' +
    '<summary>Importar uma fatura-recibo em PDF<span class="cnt off">leitura automática + cópia para o cofre</span></summary>' +
    '<div class="d-body">' +
    '<form data-form="receipt-upload" novalidate>' +
    '<div class="form-grid">' +
    '<div class="field full"><label for="fr-file">Ficheiro PDF da fatura-recibo</label>' +
    '<input id="fr-file" name="file" type="file" accept="application/pdf,.pdf" required>' +
    '<small>O PDF é copiado para o cofre (identificado pelo hash, sem alterar o original) e o painel ' +
    'mostra o que conseguiu ler. Nada entra no livro de faturas antes de confirmares.</small></div>' +
    '</div>' +
    '<div class="form-actions">' +
    '<button type="submit" class="btn btn-primary">Ler o documento</button>' +
    '<span class="note">A leitura é feita aqui, neste computador. O ficheiro não sai daqui.</span>' +
    '</div>' +
    '<div data-role="form-message"></div>' +
    '</form>' +
    '</div>' +
    '</details>'
  );
}

/** O que o servidor leu, lado a lado com o que a pessoa confirma. */
function receiptReview(receipt) {
  const draft = receipt.draft ?? {};
  const added = receipt.added ?? {};
  const read = (draft.fields ?? [])
    .map(
      (field) =>
        '<div class="rr-row">' +
        `<span class="rr-k">${esc(field.label)}</span>` +
        (field.value === null
          ? '<span class="rr-v missing">não encontrado</span>'
          : `<span class="rr-v">${esc(field.value)}</span>`) +
        (field.hint === null || field.hint === undefined ? '' : `<span class="rr-hint">${esc(field.hint)}</span>`) +
        '</div>',
    )
    .join('');

  const checks = (draft.checks ?? [])
    .map(
      (check) =>
        `<li class="chk ${check.ok ? 'ok' : 'bad'}">` +
        `<span class="g" aria-hidden="true">${check.ok ? '✓' : '✗'}</span>` +
        `<span><strong>${esc(check.label)}</strong><em>${esc(check.detail)}</em></span>` +
        '</li>',
    )
    .join('');

  const problems = draft.problems ?? [];
  const blocking = problems.length > 0 || receipt.issuerMatchesProfile === false;

  const field = (name, label, value, hint, extra = '') =>
    '<div class="field' + (extra === 'full' ? ' full' : '') + '">' +
    `<label for="rc-${name}">${esc(label)}</label>` +
    `<input id="rc-${name}" name="${name}" type="text" autocomplete="off" value="${esc(value ?? '')}">` +
    (hint === null ? '' : `<small>${esc(hint)}</small>`) +
    '</div>';

  const treatment = draft.ivaRateBp !== null && draft.ivaRateBp > 0 ? 'iva_pt' : 'isento_art53';
  const treatmentOptions = [
    ['iva_pt', 'IVA português (23%, 13% ou 6%)'],
    ['isento_art53', 'Isento pelo art. 53.º do CIVA'],
    ['autoliquidacao_ue', 'Autoliquidação — cliente noutro país da UE'],
    ['exportacao', 'Exportação — cliente fora da UE'],
  ];

  return (
    '<section class="receipt" aria-label="Conferir a leitura do documento">' +
    '<div class="rc-head">' +
    '<h3>Conferir antes de registar</h3>' +
    `<span class="rc-file mono">${esc(added.file ?? '')}</span>` +
    '</div>' +

    (blocking
      ? '<div class="banner banner-err"><span class="b-t">Este documento não pode ser registado tal como está</span>' +
        '<span class="b-d">' +
        esc(
          receipt.issuerMatchesProfile === false
            ? 'O PDF foi emitido por outra pessoa, não por ti. Uma fatura que recebeste é uma despesa, e o livro de despesas ainda não existe nesta aplicação. O PDF já ficou guardado no cofre.'
            : problems.join(' '),
        ) +
        '</span></div>'
      : '') +

    '<div class="rc-cols">' +
    '<div class="rc-read">' +
    '<h4>O que o documento diz</h4>' +
    '<div class="rr-grid">' + read + '</div>' +
    (checks === ''
      ? ''
      : '<h4 class="mt-12">Verificações do documento</h4><ul class="checks">' + checks + '</ul>') +
    '</div>' +

    '<div class="rc-confirm">' +
    '<h4>O que fica registado</h4>' +
    '<form data-form="receipt-record" novalidate>' +
    '<input type="hidden" name="documentFile" value="' + esc(added.file ?? '') + '">' +
    '<div class="form-grid">' +
    field('number', 'Número do documento', draft.number, 'Como está no PDF.') +
    field('date', 'Data de emissão', draft.date, 'Formato AAAA-MM-DD.') +
    field('clientName', 'Cliente', draft.customer?.name, null) +
    field('clientNif', 'NIF do cliente', draft.customer?.nif, null) +
    '<div class="field"><label for="rc-clientCountry">País do cliente</label>' +
    '<input id="rc-clientCountry" name="clientCountry" type="text" maxlength="2" autocomplete="off" spellcheck="false" value="' +
    esc(draft.customerCountryHint ?? '') + '">' +
    '<small>Código de duas letras (PT, ES, FR…). Decide o IVA e a retenção.</small></div>' +
    field('atcud', 'ATCUD', draft.atcud, 'Código único do documento.') +
    field(
      'baseEuros',
      'Valor ilíquido (euros)',
      draft.baseCents === null ? '' : centsToInput(draft.baseCents),
      'Sem IVA. É a base do IVA e da retenção.',
    ) +
    field(
      'ivaPercent',
      'Taxa de IVA (%)',
      draft.ivaRateBp === null ? '' : bpToInput(draft.ivaRateBp),
      'Como está no documento.',
    ) +
    field(
      'retentionPercent',
      'Retenção na fonte (%)',
      draft.retentionCents === null || draft.baseCents === null || draft.baseCents === 0
        ? '0'
        : bpToInput(Math.round((draft.retentionCents / draft.baseCents) * 10000)),
      '0 se não houve retenção.',
    ) +
    '<div class="field"><label for="rc-vatTreatment">Tratamento de IVA</label>' +
    '<select id="rc-vatTreatment" name="vatTreatment">' +
    treatmentOptions
      .map(
        ([value, label]) =>
          `<option value="${value}"${value === treatment ? ' selected' : ''}>${esc(label)}</option>`,
      )
      .join('') +
    '</select></div>' +
    '<div class="field full"><label for="rc-description">Descrição</label>' +
    '<input id="rc-description" name="description" type="text" autocomplete="off" value="' +
    esc(draft.description ?? '') + '"></div>' +
    '</div>' +
    '<div class="form-actions">' +
    '<button type="submit" class="btn btn-primary"' + (blocking ? ' disabled' : '') + '>Registar no livro de faturas</button>' +
    '<button type="button" class="btn" data-act="discard-receipt">Descartar a leitura</button>' +
    '<span class="note">' +
    (draft.declaresPaid === true
      ? 'O documento diz que foi pago: fica registado como pago, com o PDF como comprovativo.'
      : 'Se corrigires algum valor, a diferença entre o PDF e o registo fica no registo de auditoria.') +
    '</span>' +
    '</div>' +
    '<div data-role="form-message"></div>' +
    '</form>' +
    '</div>' +
    '</div>' +
    '</section>'
  );
}

/**
 * Os três blocos do arquivo manual: escolher um ficheiro, indicar um caminho
 * absoluto para um ficheiro que já está nesta máquina, e a lista do que já lá
 * está.
 */
/**
 * O campo "obrigação associada" de um documento.
 *
 * Era uma caixa de texto a pedir "o id da regra", o que exigia saber de cor que se
 * escreve `iva.dp.trimestral` — e uma gralha não dava erro nenhum: o documento
 * ficava no cofre e a regra continuava por documentar, sem que nada o dissesse.
 * Passa a ser a lista das regras que o pacote declara, com as que continuam sem
 * documentação à cabeça, porque são essas que a pessoa está ali para resolver.
 */
function obligationField(model, idPrefix) {
  const missing = rulesMissingDocumentation(model);
  const rules = model.rules?.obligations ?? [];
  const option = (rule) =>
    `<option value="${esc(rule.id)}">${esc(rule.id)} — ${esc(rule.title)}</option>`;
  const open = rules.filter((rule) => missing.has(rule.id)).map(option).join('');
  const rest = rules.filter((rule) => !missing.has(rule.id)).map(option).join('');

  return (
    `<div class="field"><label for="${idPrefix}-obligation">Obrigação associada</label>` +
    `<select id="${idPrefix}-obligation" name="obligationId">` +
    '<option value="">— sem obrigação associada —</option>' +
    (open === '' ? '' : `<optgroup label="Sem documentação no cofre">${open}</optgroup>`) +
    (rest === '' ? '' : `<optgroup label="Outras regras do pacote">${rest}</optgroup>`) +
    '</select>' +
    '<small>Associar o documento à regra é o que faz o painel deixar de o contar como em falta.</small></div>'
  );
}

function vaultArchiveForms(entries, model) {
  const upload =
    '<details class="disclosure noprint" open>' +
    '<summary>Escolher um ficheiro deste computador<span class="cnt off">o servidor calcula o SHA-256 e copia</span></summary>' +
    '<div class="d-body">' +
    '<form data-form="document-upload" novalidate>' +
    '<div class="form-grid">' +
    '<div class="field full"><label for="du-file">Ficheiro</label>' +
    '<input id="du-file" name="file" type="file" required>' +
    '<small>O ficheiro é enviado para o servidor local (loopback), identificado por hash e copiado para o cofre. ' +
    'O original fica onde está e não é alterado.</small></div>' +
    '<div class="field"><label for="du-kind">Tipo de documento</label>' +
    '<select id="du-kind" name="kind">' +
    '<option value="declaracao">Declaração</option>' +
    '<option value="guia">Guia de pagamento</option>' +
    '<option value="comprovativo">Comprovativo</option>' +
    '<option value="fatura">Fatura ou recibo</option>' +
    '<option value="saft">SAF-T</option>' +
    '<option value="contrato">Contrato</option>' +
    '<option value="outro" selected>Outro</option>' +
    '</select></div>' +
    obligationField(model, 'du') +
    '</div>' +
    '<div class="form-actions">' +
    '<button type="submit" class="btn btn-primary">Guardar no cofre</button>' +
    '</div>' +
    '<div data-role="form-message"></div>' +
    '</form>' +
    '</div>' +
    '</details>';

  const form =
    '<details class="disclosure noprint">' +
    '<summary>Registar documento por caminho absoluto<span class="cnt off">para ficheiros que já estão nesta máquina</span></summary>' +
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
    obligationField(model, 'd') +
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
      'Ainda não há documentos no cofre',
      'Guarda aqui um comprovativo, uma guia ou um contrato. Associar o documento à obrigação ' +
        'correspondente é o que faz o painel deixar de a contar como pendente.',
    );
  }

  return { upload, form, table };
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
      'Todo o cálculo desta aplicação sai daqui, e cada regra diz de que lei vem e até onde foi ' +
        'confirmada. As que ainda não estão verificadas aparecem como tal: um prazo por confirmar é ' +
        'uma informação, não uma certeza.',
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
   11. Diagnóstico — o mesmo que `vnfin doctor` responde, para quem só usa o
   browser.

   Nada aqui é calculado: o modelo traz o perfil, o pacote de regras, as
   contagens do cofre, o risco da pasta de dados e o estado da chave. Esta
   secção reúne-os num só lugar, e é onde vive o formulário da chave da API,
   porque era o único passo que ainda obrigava a abrir um terminal.
   -------------------------------------------------------------------------- */

const KEY_SOURCE_LABEL = {
  flag: 'indicada no arranque',
  env: 'variável de ambiente',
  file: 'ficheiro cifrado no cofre',
  session: 'esta sessão do painel',
  none: 'sem origem',
};

/** A chave da API: guardar, desbloquear, esquecer ou apagar — tudo daqui. */
function aiKeyPanel(model) {
  const key = model.key ?? { available: false, source: 'none', masked: null, problems: [], stored: false, locked: false, passphraseFromEnv: false, minPassphraseLength: 12 };
  const min = Number(key.minPassphraseLength ?? 12);

  const stateLine = key.available
    ? '<p class="dl-inline"><span class="state on"><span aria-hidden="true">●</span> Chave em uso</span>' +
      `<span class="mono">${esc(key.masked ?? '••••')}</span>` +
      `<span class="badge local">${esc(KEY_SOURCE_LABEL[key.source] ?? key.source)}</span></p>`
    : key.locked
      ? '<p class="dl-inline"><span class="state off"><span aria-hidden="true">⊘</span> Chave guardada, fechada</span>' +
        '<span class="badge miss">falta a frase-passe</span></p>'
      : '<p class="dl-inline"><span class="state off"><span aria-hidden="true">⊘</span> Sem chave configurada</span>' +
        '<span class="badge miss">assistente desligado</span></p>';

  const unlock =
    key.locked && !key.available
      ? '<form data-form="ai-key-unlock" novalidate class="mt-10">' +
        '<div class="form-grid">' +
        '<div class="field full"><label for="ak-pass-unlock">Frase-passe da chave guardada</label>' +
        '<input id="ak-pass-unlock" name="passphrase" type="password" autocomplete="current-password" required>' +
        `<small>A mesma frase-passe com que a chave foi cifrada (${esc(min)} caracteres ou mais). É usada para abrir a chave e não é guardada.</small></div>` +
        '</div>' +
        '<div class="form-actions"><button type="submit" class="btn btn-primary">Desbloquear nesta sessão</button></div>' +
        '<div data-role="form-message"></div>' +
        '</form>'
      : '';

  const replace =
    '<details class="disclosure noprint"' + (key.available ? '' : ' open') + '>' +
    '<summary>' + (key.available ? 'Substituir a chave' : 'Configurar a chave DeepSeek') +
    '<span class="cnt off">fica fora do browser</span></summary>' +
    '<div class="d-body">' +
    '<p class="tnote">A chave é enviada para o servidor local por loopback, cifrada no cofre se indicares uma frase-passe, ' +
    'e nunca é devolvida ao browser — daqui só sai uma máscara. Sem frase-passe a chave vive apenas na memória deste ' +
    'processo e desaparece quando fechas o painel.</p>' +
    '<form data-form="ai-key" novalidate>' +
    '<div class="form-grid">' +
    '<div class="field full"><label for="ak-key">Chave da API DeepSeek</label>' +
    '<input id="ak-key" name="key" type="password" autocomplete="off" spellcheck="false" placeholder="sk-…" required>' +
    '<small>Não é validada aqui: uma chave errada só se revela no primeiro envio. Não é escrita no registo de auditoria.</small></div>' +
    '<div class="field full"><label for="ak-pass">Frase-passe para a guardar cifrada (opcional)</label>' +
    `<input id="ak-pass" name="passphrase" type="password" autocomplete="new-password">` +
    `<small>Com ${esc(min)} caracteres ou mais, a chave fica cifrada no cofre e sobrevive ao reinício. Vazio: só nesta sessão.</small></div>` +
    '</div>' +
    '<div class="form-actions">' +
    '<button type="submit" class="btn btn-primary">Usar esta chave</button>' +
    (key.source === 'session'
      ? '<button type="button" class="btn" data-act="ai-key-forget">Esquecer a chave desta sessão</button>'
      : '') +
    (key.stored
      ? '<button type="button" class="btn btn-tertiary" data-act="ai-key-delete">Apagar a chave guardada no cofre</button>'
      : '') +
    '</div>' +
    '<div data-role="form-message"></div>' +
    '</form>' +
    '</div>' +
    '</details>';

  const envNote =
    key.stored === true && key.passphraseFromEnv === true
      ? '<p class="tnote"><span class="state on"><span aria-hidden="true">✓</span> Frase-passe no ambiente</span> ' +
        'A frase-passe está definida no ambiente do servidor (<span class="mono">VN_FINANCE_PASSPHRASE</span>), ' +
        'por isso a chave guardada no cofre abre sozinha quando o painel arranca.</p>'
      : '';

  return (
    '<div class="panel mt-10 panel-pad">' +
    '<p class="subhead">Assistente — chave da API</p>' +
    stateLine +
    envNote +
    notes(key.problems ?? [], 'danger') +
    unlock +
    replace +
    '</div>'
  );
}

function renderDiagnostico(model) {
  const meta = model.meta ?? {};
  const summary = model.pack?.summary ?? {};
  const freshness = model.pack?.freshness ?? null;
  const key = model.key ?? {};
  const profile = model.profile ?? null;
  const vault = model.vault ?? { files: 0, bytes: 0, entries: [] };
  const risk = meta.dataDirRisk ?? { level: 'none', message: null };

  const profileProblems = model.profileProblems ?? [];
  const profileErrors = profileProblems.filter((problem) => problem.level === 'error');
  const profileWarnings = profileProblems.filter((problem) => problem.level === 'warning');
  const packProblems = (model.pack?.problems ?? []).filter((problem) => problem.level !== 'info');
  const packHealth = (model.flags ?? []).filter((flag) => flag.code === 'RULE_PACK_STALE' || flag.code === 'VALUE_PROPOSED_BY_AI_UNCONFIRMED');
  const nullConstants = summary.nullConstants ?? [];
  const missingInputs = model.missingInputs ?? [];

  const unverified = (summary.unverified ?? 0) + (summary.stale ?? 0);
  const problems =
    profileErrors.length +
    packProblems.filter((problem) => problem.level === 'error').length +
    (risk.level === 'none' ? 0 : 1) +
    (freshness !== null && freshness.status !== 'current' ? 1 : 0) +
    unverified;
  const warnings = profileWarnings.length + missingInputs.length + nullConstants.length + packHealth.length + packProblems.filter((p) => p.level === 'warning').length;

  const verdict =
    problems === 0 && warnings === 0
      ? '<div class="disc pos"><span class="sev sev-info"><span class="g" aria-hidden="true">✓</span> Sem problemas</span>' +
        '<div><p class="a-t">Nada a assinalar</p><p class="a-d">Perfil, pacote de regras, cofre e chave estão como deviam estar.</p></div></div>'
      : `<div class="disc"><span class="sev ${problems > 0 ? 'sev-alta' : 'sev-media'}"><span class="g" aria-hidden="true">▲</span> ` +
        `${esc(problems)} erro(s) · ${esc(warnings)} aviso(s)</span>` +
        '<div><p class="a-t">O que fazer a seguir</p><p class="a-d">Cada ponto abaixo diz o que se passa e o que falta. ' +
        'Um aviso não impede o trabalho; um erro impede um cálculo.</p></div></div>';

  const envTable = tableWrap(
    'Estado do ambiente local: aplicação, runtime, cofre, pacote de regras e chave da API.',
    '<th scope="col">Item</th><th scope="col">Valor</th>',
    [
      '<tr><td>Versão da aplicação</td><td class="mono">' + esc(meta.version ?? '—') + '</td></tr>',
      '<tr><td>Runtime</td><td class="mono">Node ' + esc(meta.nodeVersion ?? '—') + '</td></tr>',
      '<tr><td>Exercício</td><td class="mono">' + esc(meta.year ?? '—') + ' · hoje ' + esc(ptDate(meta.today)) + '</td></tr>',
      '<tr><td>Cofre</td><td class="mono">' + esc(meta.dataDir ?? '—') + '</td></tr>',
      '<tr><td>Ficheiros no cofre</td><td>' + esc(vault.files) + ' ficheiros · ' + esc(bytesLabel(vault.bytes)) + '</td></tr>',
      '<tr><td>Pacote de regras</td><td>pt/' + esc(meta.packVersion ?? '—') + ' <span class="mono">' + esc(String(meta.packChecksum ?? '').slice(0, 16)) + '…</span><span class="sub mono">' + esc(meta.packPath ?? '') + '</span></td></tr>',
      '<tr><td>Chave DeepSeek</td><td>' +
        (key.available
          ? '<span class="badge local">presente</span> <span class="mono">' + esc(key.masked ?? '••••') + '</span> <span class="sub">' + esc(KEY_SOURCE_LABEL[key.source] ?? key.source ?? '') + '</span>'
          : '<span class="badge miss">ausente</span><span class="sub">o assistente fica indisponível; todo o resto funciona</span>') +
        '</td></tr>',
    ].join(''),
  );

  const riskBlock =
    risk.level === 'none'
      ? ''
      : `<div class="disc noprint"><span class="sev ${risk.level === 'fatal' ? 'sev-alta' : 'sev-media'}">` +
        `<span class="g" aria-hidden="true">▲</span> Cofre ${risk.level === 'fatal' ? 'recusado' : 'em git'}</span>` +
        `<div><p class="a-t">A pasta de dados ${risk.level === 'fatal' ? 'está dentro do repositório da aplicação' : 'está dentro de um repositório git'}</p>` +
        `<p class="a-d">${escLines(risk.message ?? '')}</p></div></div>`;

  const profileBlock =
    profile === null
      ? emptyState('Sem perfil', 'Não existe perfil neste cofre. O painel abre o formulário de perfil no primeiro ecrã — nenhum comando é necessário.')
      : (profileProblems.length === 0
          ? '<p class="tnote"><span class="state on"><span aria-hidden="true">✓</span> Perfil válido</span> ' +
            esc(profile.name) + ' · NIF ' + esc(maskNif(String(profile.nif ?? ''))) + ' · IVA ' + esc(ivaRegimeLabel(profile.iva?.regime)) + '</p>'
          : notes(
              profileProblems.map((problem) => `${problem.level === 'error' ? 'erro' : 'aviso'} · ${problem.field}: ${problem.message}`),
              profileErrors.length > 0 ? 'danger' : 'warn',
            )) +
        (missingInputs.length === 0
          ? ''
          : '<p class="subhead mt-12">Dados que só tu podes fornecer</p>' +
            notes(missingInputs) +
            '<p class="tnote">A aplicação não adivinha estes valores: sem eles há verificações que simplesmente não correm.</p>');

  // The validator's warnings and the pack's own "todo" list are the subject of
  // "Regras e fontes", which prints both in full. Printing them here too put the
  // same four warnings and nineteen notes on two pages at once, so the diagnostic
  // states the count and links to the page that explains it. Errors still print
  // inline: a warning does not stop a calculation, an error does.
  const packErrors = packProblems.filter((problem) => problem.level === 'error');
  const packWarningCount = packProblems.length - packErrors.length;
  const reviewPoints = (model.rules?.todo ?? []).length;
  const packAuditNote =
    packWarningCount === 0 && reviewPoints === 0
      ? ''
      : '<p class="tnote">' +
        esc(packWarningCount) +
        (packWarningCount === 1 ? ' aviso do validador' : ' avisos do validador') +
        (reviewPoints === 0
          ? ''
          : ' e ' + esc(reviewPoints) + (reviewPoints === 1 ? ' ponto por confirmar' : ' pontos por confirmar')) +
        ', com o detalhe em <a href="#regras">Regras e fontes</a>.</p>';

  const packBlock =
    (freshness === null
      ? ''
      : `<p class="tnote">${freshness.status === 'current' ? '<span class="state on"><span aria-hidden="true">✓</span> Pacote atual</span>' : '<span class="state off"><span aria-hidden="true">▲</span> Pacote a rever</span>'} ${esc(freshness.message)}</p>`) +
    `<p class="tnote">${esc(summary.verified ?? 0)} regras verificadas · ${esc(summary.partial ?? 0)} parciais · ` +
    `${esc(unverified)} por verificar ou desatualizadas · ${esc(summary.eventDriven ?? 0)} sem prazo fixo (não entram na agenda)` +
    ((summary.nullConstants ?? []).length === 0
      ? ''
      : ` · ${esc((summary.nullConstants ?? []).length)} constantes sem valor (os cálculos que dependem delas são recusados, não estimados)`) +
    '.</p>' +
    notes(packErrors.slice(0, 20).map((problem) => `${problem.path} — ${problem.message}`), 'danger') +
    packAuditNote +
    (packHealth.length === 0
      ? ''
      : '<p class="subhead mt-12">Saúde do pacote de regras</p>' +
        notes(packHealth.map((flag) => `${flag.title} — ${String(flag.detail).split('\n')[0] ?? ''}`)));

  return (
    '<section class="block" id="diagnostico" aria-label="Diagnóstico">' +
    sectionHead(
      'Diagnóstico',
      'O mesmo que o comando <span class="mono">doctor</span> responde: perfil, pacote de regras, cofre e chave',
      'Se alguma coisa não estiver a funcionar — um valor em falta, uma pasta em risco, uma chave por ' +
        'abrir — é aqui que se vê o que se passa. Tudo o que os comandos de terminal diriam está nesta ' +
        'secção, porque quem usa o painel não deve ter de abrir um terminal para saber isto.',
    ) +
    verdict +
    envTable +
    riskBlock +
    '<p class="subhead mt-14">Perfil</p>' +
    profileBlock +
    '<p class="subhead mt-14">Pacote de regras e cofre</p>' +
    packBlock +
    aiKeyPanel(model) +
    '<p class="tnote">Este diagnóstico é lido do cofre local e do pacote de regras em cada carregamento da página. ' +
    'Nada é enviado para fora deste computador: a única chamada de rede da aplicação é a que autorizares em «Atualizar regras».</p>' +
    '</section>'
  );
}

/* --------------------------------------------------------------------------
   12. Atualizar regras (o assistente limitado)
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
        `<span class="badge local">${esc(KEY_SOURCE_LABEL[update.keySource] ?? update.keySource ?? 'origem não indicada')}</span></span>` +
        '<small>A chave nunca é escrita no registo de auditoria nem devolvida ao browser — só a máscara. ' +
        'Podes substituí-la no <a href="#diagnostico">Diagnóstico</a>.</small></dd></div>'
      : '<div><dt>Chave de API</dt><dd><span class="state off"><span aria-hidden="true">⊘</span> Sem chave configurada</span>' +
        '<small>O envio está desativado. Guarda uma chave no <a href="#diagnostico">Diagnóstico</a>, sem sair do painel.</small></dd></div>';

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
            '<span class="cta-note">Envio desativado: não há chave configurada. ' +
            'Configura-a no <a href="#diagnostico">Diagnóstico</a>.</span>') +
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
        : '<button type="button" class="btn" disabled>Pré-visualizar pedido</button><span class="cta-note">Disponível quando existir chave: configura-a no <a href="#diagnostico">Diagnóstico</a>.</span>') +
    '</div>' +
    '</div>';

  return (
    '<section class="block" id="assistente" aria-label="Atualização das regras">' +
    sectionHead(
      'Atualizar regras',
      'A única função do assistente: propor os valores que a lei muda de ano para ano. Não conversa, não aconselha e não vê os teus dados.',
      'Os prazos e as taxas mudam todos os anos. Em vez de os escrever à mão no pacote de regras, ' +
        'podes pedir a um modelo que os proponha — e a proposta fica por confirmar até tu a validares. ' +
        'É o único momento em que algo sai deste computador, e sai sem nada teu lá dentro.',
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
    `<label class="check" for="pf-eu50k"><input id="pf-eu50k" name="intraCommunityOperationsAbove50k" type="checkbox"${activity.intraCommunityOperationsAbove50k === true ? ' checked' : ''}> <span>Essas operações passaram 50 000 EUR num trimestre (a declaração recapitulativa passa a mensal, mesmo no regime trimestral)</span></label>` +
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
    intraCommunityOperationsAbove50k: fieldChecked(form, 'intraCommunityOperationsAbove50k'),
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
    sectionHead(
      'Primeiros passos',
      'O painel não inventa dados',
      'Faltam os dados que só tu tens: os da tua declaração de início de atividade. Enquanto não ' +
        'estiverem aqui, a aplicação prefere não mostrar nada a mostrar estimativas inventadas.',
    ) +
    '<div class="onboard">' +
    '<h2>Ainda não existe perfil neste cofre</h2>' +
    '<p>O painel lê o perfil, as faturas registadas e o pacote de regras do cofre local. Sem perfil não há enquadramento, não há agenda e não há indicadores — ' +
    'e é deliberado que assim seja: um painel que preenche estes valores por estimativa seria um painel em que não se pode confiar.</p>' +
    '<p>O formulário de perfil tem os dados da tua declaração de início de atividade: NIF, nome, regime de IVA e a data de abertura. ' +
    'Sem eles a aplicação não sabe que obrigações te pertencem, e não os adivinha.</p>' +
    '<ul>' +
    '<li>O cofre fica em <span class="mono">' + esc(model.meta.dataDir) + '</span> e nunca sai deste computador. Podes abrir a pasta e mudá-la na secção «Cofre de documentos».</li>' +
    '<li>O volume de negócios do ano anterior nunca é assumido: sem ele, a aplicação não avalia a isenção do art. 53.º do CIVA e diz que não avalia.</li>' +
    '<li>Um perfil já existente pode ser carregado de um ficheiro <span class="mono">profile.json</span>, no mesmo formulário.</li>' +
    '<li>Depois do perfil criado, esta página passa a mostrar enquadramento, agenda fiscal, faturas, Segurança Social, IVA e IRS.</li>' +
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
   Sobre
   -------------------------------------------------------------------------- */

/**
 * O que a aplicação não faz, numa página em vez de num rodapé.
 *
 * A lista não é escrita aqui: vem do modelo, do mesmo sítio de onde a linha de
 * comandos e a documentação a leem. Se um dia deixar de ser verdade, é uma
 * avaria num sítio só — e não duas versões da mesma promessa a divergir.
 */
function renderAbout(model) {
  const guarantees = model.guarantees ?? [];
  const lock =
    '<svg viewBox="0 0 16 16" aria-hidden="true"><rect x="3" y="7" width="10" height="7" rx="1.2"/>' +
    '<path d="M5.6 7V5.2a2.4 2.4 0 014.8 0V7"/></svg>';

  return (
    '<section class="block" id="about" aria-label="Sobre esta aplicação">' +
    sectionHead(
      'About',
      null,
      'Nada desta lista é uma funcionalidade por construir: são decisões, e nenhuma delas depende de ' +
        'configuração. É também a razão pela qual não há aqui nada para autorizar, sincronizar ou manter ' +
        'ligado à Internet.',
    ) +
    '<p class="subhead">O que esta aplicação não faz</p>' +
    '<ul class="about-list">' +
    guarantees.map((item) => `<li>${esc(item)}</li>`).join('') +
    '</ul>' +
    '<p class="about-warn">Esta aplicação não substitui um contabilista certificado e não entrega declarações por ti.</p>' +
    '<p class="about-local">' +
    lock +
    '<span>Todos os dados residem neste computador · sem telemetria · sem servidores · painel local v' +
    esc(model.meta.version) +
    '</span>' +
    '</p>' +
    '</section>'
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

  // Primeiro o sítio onde os dados vivem, depois os dados. Uma instalação nova
  // que começa a guardar a vida financeira de alguém numa pasta escondida da
  // pasta pessoal está a tomar uma decisão que não foi de ninguém; por isso a
  // pergunta vem antes de tudo o resto, e só quando ainda não há nada.
  if (needsVaultChoice(model)) {
    state.vault.asked = true;
    renderProfileModal(model);
    host.innerHTML = renderVaultSetup(model) + renderDiagnostico(model);
    return;
  }

  // First run: there is nothing to show until the profile exists, so the form is
  // what opens. Asked once per page, so closing it to read the rules does not
  // have it spring back on the next render — and only on the Resumo page, which
  // is the normal way in: landing on `#regras` from a link and being covered by a
  // form is worse than a card that says what is missing.
  const page = pageById(state.page);
  const hasProfile = model.profile !== null && model.profile !== undefined;
  if (!hasProfile && !state.profilePrompted) {
    state.profilePrompted = true;
    if (page.id === 'painel') state.profileOpen = true;
  }
  renderProfileModal(model);

  const html = [];

  if (!hasProfile && PROFILE_PAGES.has(page.id)) {
    // Uma página que só mostra zeros é pior do que uma página que explica o que
    // falta: o painel não inventa um perfil, diz que precisa dele.
    html.push(renderNeedsProfile(page));
  } else if (page.id === 'painel') {
    html.push(renderAlertBlock(model, { page: page.id }));
    if (hasProfile) {
      html.push(renderKpis(model));
      html.push(renderEnquadramento(model));
    } else {
      html.push(renderOnboarding(model));
    }
  } else if (page.id === 'agenda') {
    html.push(renderAgenda(model));
  } else if (page.id === 'recibos') {
    html.push(renderRecibos(model));
  } else if (page.id === 'ss') {
    html.push(renderSs(model));
  } else if (page.id === 'iva') {
    html.push(renderIva(model));
  } else if (page.id === 'irs') {
    html.push(renderIrs(model));
  } else if (page.id === 'cofre') {
    html.push(renderCofre(model));
  } else if (page.id === 'regras') {
    html.push(renderRegras(model));
  } else if (page.id === 'diagnostico') {
    html.push(renderDiagnostico(model));
  } else if (page.id === 'assistente') {
    html.push(renderAssistente(model));
  } else if (page.id === 'about') {
    html.push(renderAbout(model));
  }

  // O aviso de alertas urgentes aparece em qualquer página: um prazo em atraso
  // não deixa de existir por se estar a olhar para outra coisa.
  if (page.id !== 'painel' && hasProfile) html.unshift(renderAlertBlock(model, { page: page.id }));

  host.innerHTML = html.join('');
}

/**
 * A página que precisa de um perfil, e o caminho para o criar.
 *
 * Substitui os zeros que o modelo devolve quando não há perfil: uma tabela de
 * trimestres a zero parece um resultado, e não é — é a ausência de um dado que
 * só a pessoa tem.
 */
function renderNeedsProfile(page) {
  return (
    '<section class="block" aria-label="Perfil necessário">' +
    sectionHead(page.label, 'Falta o perfil do contribuinte') +
    '<div class="onboard">' +
    `<h2>Esta página precisa do perfil para calcular «${esc(page.label)}»</h2>` +
    '<p>O IVA, a Segurança Social, a agenda e o rendimento tributável saem todos da mesma base: os dados ' +
    'da tua declaração de início de atividade e as faturas que registares. Sem perfil, o painel não sabe ' +
    'a que regime estás sujeito nem que obrigações te pertencem.</p>' +
    '<p>Não é um erro nem uma avaria: é o painel a recusar-se a mostrar zeros que pareceriam resultados.</p>' +
    '<div class="acts">' +
    '<button type="button" class="btn btn-primary" data-act="open-profile">Preencher o perfil</button>' +
    '<a class="btn" href="#regras">Ver as regras e as fontes</a>' +
    '</div>' +
    '</div>' +
    '</section>'
  );
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

/**
 * Como `fieldValue`, mas sem tirar espaços.
 *
 * Só a frase-passe da chave precisa disto: é um segredo escrito por uma pessoa e
 * um espaço no início ou no fim é parte dele. Apará-lo aqui faria com que uma
 * chave guardada na linha de comandos não abrisse no painel.
 */
function fieldRaw(form, name) {
  const field = form.elements.namedItem(name);
  return field !== null && typeof field.value === 'string' ? field.value : '';
}

function fieldChecked(form, name) {
  const field = form.elements.namedItem(name);
  return field !== null && field.checked === true;
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
  body.intraCommunityOperationsAbove50k = fieldChecked(form, 'intraCommunityOperationsAbove50k');
  body.exports = fieldChecked(form, 'exports');

  await send(API.profile, body);
  toast('Enquadramento atualizado no cofre local.');
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

/**
 * O mesmo registo, para o ficheiro escolhido no browser.
 *
 * O ficheiro vai como corpo do pedido (`application/octet-stream`) e o nome, o
 * tipo e a obrigação vão na query: um browser não sabe o caminho absoluto de um
 * ficheiro que o utilizador escolheu, e inventá-lo seria pior do que não o ter.
 */
async function submitDocumentUpload(form) {
  const input = form.querySelector('input[type="file"]');
  const file = input === null || input.files === null ? null : input.files[0];
  if (file === null) {
    formMessage(form, 'err', 'Escolhe um ficheiro para guardar no cofre.');
    return;
  }

  const params = new URLSearchParams();
  params.set('name', file.name);
  const kind = fieldValue(form, 'kind');
  if (kind !== '') params.set('kind', kind);
  const obligationId = fieldValue(form, 'obligationId');
  if (obligationId !== '') params.set('obligationId', obligationId);

  const data = await sendBinary(`${API.documentsUpload}?${params.toString()}`, file);
  const added = data.added ?? null;
  toast(
    added === null
      ? 'Documento guardado no cofre.'
      : `Documento no cofre: ${added.file} · sha256 ${String(added.sha256 ?? '').slice(0, 16)}…`,
  );
  if (input !== null) input.value = '';
}

/* --------------------------------------------------------------------------
   Fatura-recibo em PDF: enviar, conferir, registar.
   -------------------------------------------------------------------------- */

/**
 * Enviar o PDF para leitura.
 *
 * Duas coisas acontecem de uma vez, e é de propósito: o ficheiro é arquivado no
 * cofre (a cópia é sempre segura — é o documento que a pessoa quis guardar) e o
 * servidor diz o que conseguiu ler. O livro de faturas não é tocado: registrar
 * uma fatura é uma decisão, e a decisão é o passo seguinte.
 */
async function submitReceiptUpload(form) {
  const input = form.querySelector('input[type="file"]');
  const file = input === null || input.files === null ? null : input.files[0];
  if (file === null) {
    formMessage(form, 'err', 'Escolhe o PDF da fatura-recibo.');
    return;
  }
  if (!/\.pdf$/i.test(file.name) && file.type !== 'application/pdf') {
    formMessage(form, 'err', 'Este passo lê faturas em PDF. Para outro tipo de ficheiro, usa «Guardar um documento no cofre».');
    return;
  }

  const data = await sendBinary(`${API.receiptsUpload}?name=${encodeURIComponent(file.name)}`, file);
  state.receipt = {
    draft: data.draft ?? {},
    added: data.added ?? {},
    issuerMatchesProfile: data.issuerMatchesProfile ?? null,
  };
  if (input !== null) input.value = '';
  // O modelo já foi aplicado pelo sendBinary: este render acrescenta o cartão de
  // conferência, que vive no estado.
  if (state.model !== null) applyModel(state.model);
  const host = document.getElementById('cofre');
  if (host !== null) host.scrollIntoView({ behavior: 'smooth', block: 'start' });
  const found = (state.receipt.draft.fields ?? []).filter((field) => field.value !== null).length;
  toast(`PDF guardado no cofre. Campos lidos: ${found} de ${(state.receipt.draft.fields ?? []).length}.`);
}

/** Registar a fatura confirmada: os valores do formulário são a declaração. */
async function submitReceiptRecord(form) {
  const documentFile = fieldValue(form, 'documentFile');
  if (documentFile === '') {
    formMessage(form, 'err', 'O documento já não está identificado. Lê o PDF outra vez.');
    return;
  }

  const body = { documentFile };

  const text = (name) => fieldValue(form, name);
  if (text('number') !== '') body.number = text('number');
  if (text('date') !== '') body.date = text('date');
  if (text('clientName') !== '') body.clientName = text('clientName');
  if (text('clientNif') !== '') body.clientNif = text('clientNif');
  if (text('clientCountry') !== '') body.clientCountry = text('clientCountry').toUpperCase().slice(0, 2);
  if (text('description') !== '') body.description = text('description');
  if (text('atcud') !== '') body.atcud = text('atcud');
  if (text('vatTreatment') !== '') body.vatTreatment = text('vatTreatment');

  // As duas conversões que a interface faz, e as únicas: euros -> cêntimos e
  // percentagem -> pontos base. É o contrato da API a exigi-las.
  const base = parseEurosToCents(text('baseEuros'));
  if (base === undefined || base === null || base <= 0) {
    formMessage(form, 'err', 'Escreve o valor ilíquido em euros, por exemplo 813,01.');
    return;
  }
  body.baseCents = base;

  const iva = text('ivaPercent') === '' ? 0 : parsePercentToBp(text('ivaPercent'));
  if (iva === undefined || iva === null || iva < 0) {
    formMessage(form, 'err', 'Escreve a taxa de IVA em percentagem, por exemplo 23.');
    return;
  }
  body.ivaRateBp = iva;

  const retention = text('retentionPercent') === '' ? 0 : parsePercentToBp(text('retentionPercent'));
  if (retention === undefined || retention === null || retention < 0) {
    formMessage(form, 'err', 'Escreve a retenção em percentagem, ou 0 se não houve.');
    return;
  }
  body.retentionBp = retention;

  const data = await send(API.receiptsRecord, body);
  state.receipt = null;
  if (state.model !== null) applyModel(state.model);

  const invoice = data.invoice ?? {};
  const divergences = Array.isArray(data.divergences) ? data.divergences : [];
  const warnings = Array.isArray(data.warnings) ? data.warnings : [];
  toast(
    `Fatura ${String(invoice.number ?? '')} registada no livro.` +
      (divergences.length === 0 ? '' : ` Corrigido em relação ao PDF: ${divergences.join('; ')}`),
  );
  // Os avisos do servidor são notas, não erros: dizem o que ficou registado e
  // porque é que o documento e o registo podem não coincidir ao cêntimo.
  if (warnings.length > 0) showNotice(warnings.join('\n• '), warnings.length > 1 ? 'warn' : 'info');
}

/** Um POST cujo corpo são bytes de um ficheiro, e não JSON. */
async function sendBinary(path, blob) {
  let response;
  try {
    response = await fetch(path, {
      method: 'POST',
      headers: { 'X-VNFIN-Token': state.token, 'Content-Type': 'application/octet-stream' },
      credentials: 'omit',
      cache: 'no-store',
      body: blob,
    });
  } catch {
    throw new ApiError('Não foi possível contactar o servidor local. Confirma que ainda está a correr.');
  }
  const text = await response.text();
  let data = null;
  try {
    data = text === '' ? null : JSON.parse(text);
  } catch {
    data = null;
  }
  if (!response.ok || data === null || data.ok === false) {
    const message =
      data !== null && typeof data.error === 'string' && data.error !== ''
        ? data.error
        : `O servidor local respondeu ${response.status}${response.statusText ? ` ${response.statusText}` : ''}.`;
    throw new ApiError(message);
  }
  const model = data.model;
  if (model === null || typeof model !== 'object') {
    throw new ApiError('A resposta do servidor não incluiu o modelo atualizado.');
  }
  applyModel(model);
  return data;
}

/**
 * A chave da API, escrita no painel.
 *
 * Sem frase-passe a chave vale só para esta sessão do servidor; com ela, fica
 * cifrada no cofre e sobrevive ao reinício. As duas coisas são ditas antes e
 * depois, porque "guardei a chave" e "guardei a chave até fechar a janela" são
 * promessas diferentes.
 */
async function submitAiKey(form, unlockOnly) {
  const passphrase = fieldRaw(form, 'passphrase');

  if (unlockOnly) {
    if (passphrase === '') {
      formMessage(form, 'err', 'Escreve a frase-passe com que a chave foi cifrada.');
      return;
    }
    await send(API.aiKeyUnlock, { passphrase });
    toast('Chave desbloqueada nesta sessão. O envio para a DeepSeek está disponível.');
    return;
  }

  const key = fieldValue(form, 'key');
  if (key.trim() === '') {
    formMessage(form, 'err', 'Escreve a chave da API.');
    return;
  }
  const body = { key: key.trim() };
  if (passphrase !== '') body.passphrase = passphrase;

  const data = await send(API.aiKey, body);
  toast(
    data.stored === true
      ? 'Chave ativa e cifrada no cofre.'
      : 'Chave ativa apenas nesta sessão: o cofre não foi alterado.',
  );
}

/* A navegação entre separadores é do endereço, não do JavaScript: uma ligação
   `#cofre` num alerta, o botão "voltar" do navegador e uma ligação colada noutro
   sítio têm todas de levar ao mesmo sítio. Por isso o roteador ouve o hash, e os
   botões que mudam de página limitam-se a escrevê-lo. */
window.addEventListener('hashchange', () => {
  const route = currentRoute();
  if (route.page === state.page && route.anchor === state.anchor) return;
  applyRoute(route);
});

document.addEventListener('click', (event) => {
  if (!(event.target instanceof Element)) return;
  // Uma ligação interna é apanhada aqui para a página ser renderizada já, e não
  // no instante seguinte: o `hashchange` é assíncrono.
  const link = event.target.closest('a[href^="#"]');
  if (link !== null && link.getAttribute('href') !== '#') {
    const resolved = routeFor(String(link.getAttribute('href')));
    if (resolved.page !== state.page || resolved.anchor !== state.anchor) {
      event.preventDefault();
      navigate(resolved.page, resolved.anchor);
    }
    return;
  }
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
    navigate('agenda');
    return;
  }
  if (act === 'filter') {
    state.filter = trigger.dataset.filter ?? 'year';
    if (state.model !== null) applyModel(state.model);
    return;
  }
  if (act === 'preview-update') {
    state.preview = true;
    navigate('assistente');
    return;
  }
  if (act === 'ai-key-forget') {
    if (!window.confirm('Esquecer a chave nesta sessão? O cofre não é tocado e a próxima utilização volta a pedi-la.')) return;
    void withBusy(trigger, async () => {
      await send(API.aiKey, { forget: true });
      toast('Chave esquecida nesta sessão.');
    }).catch((error) => {
      showError(error instanceof Error ? error.message : String(error));
    });
    return;
  }
  if (act === 'ai-key-delete') {
    if (!window.confirm('Apagar a chave cifrada guardada no cofre? Fica só o que estiver no ambiente ou nesta sessão.')) return;
    void withBusy(trigger, async () => {
      const data = await send(API.aiKey, { delete: true });
      toast(data.deleted === true ? 'Chave guardada apagada do cofre.' : 'Não havia chave guardada para apagar.');
    }).catch((error) => {
      showError(error instanceof Error ? error.message : String(error));
    });
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
  if (act === 'open-vault-picker') {
    const start = state.model === null ? null : state.model.meta.dataDir;
    void openVaultPicker(start);
    return;
  }
  if (act === 'close-vault-picker') {
    closeVaultPicker();
    return;
  }
  if (act === 'vault-go') {
    void loadVaultListing(trigger.dataset.path ?? null);
    return;
  }
  if (act === 'use-default-vault') {
    const path = state.model === null ? '' : state.model.meta.dataDir;
    void withBusy(trigger, () => chooseVault(path)).catch((error) => {
      showError(error instanceof Error ? error.message : String(error));
    });
    return;
  }
  if (act === 'vault-use') {
    void withBusy(trigger, () => chooseVault(trigger.dataset.path ?? '')).catch((error) => {
      showError(error instanceof Error ? error.message : String(error));
    });
    return;
  }
  if (act === 'vault-new-folder') {
    const field = document.getElementById('vp-new-name');
    const name = field === null ? '' : String(field.value).trim();
    const base = state.vault.listing === null ? '' : String(state.vault.listing.path);
    if (name === '') {
      showError('Escreve o nome da pasta nova.');
      return;
    }
    if (/[\\/]/.test(name)) {
      showError('O nome da pasta nova não pode ter barras.');
      return;
    }
    void withBusy(trigger, () => chooseVault(joinPath(base, name))).catch((error) => {
      showError(error instanceof Error ? error.message : String(error));
    });
    return;
  }
  if (act === 'reveal-vault') {
    void revealVault(trigger).catch((error) => {
      showError(error instanceof Error ? error.message : String(error));
    });
    return;
  }
  if (act === 'discard-receipt') {
    state.receipt = null;
    if (state.model !== null) applyModel(state.model);
    toast('Leitura descartada. O PDF continua guardado no cofre.');
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
        : kind === 'document'
          ? () => submitDocument(form)
          : kind === 'document-upload'
            ? () => submitDocumentUpload(form)
            : kind === 'receipt-upload'
              ? () => submitReceiptUpload(form)
              : kind === 'receipt-record'
                ? () => submitReceiptRecord(form)
                : kind === 'ai-key'
                  ? () => submitAiKey(form, false)
                  : kind === 'ai-key-unlock'
                    ? () => submitAiKey(form, true)
                    : null;
  if (work === null) return;
  void withBusy(button, work).catch((error) => {
    formMessage(form, 'err', error instanceof Error ? error.message : String(error));
    showError(error instanceof Error ? error.message : String(error));
  });
});

document.addEventListener('change', (event) => {
  const target = event.target;
  // Um `profile.json` escolhido no disco entra pelo mesmo POST do formulário.
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
  if (event.target.id === 'vault-overlay') closeVaultPicker();
});

document.addEventListener('keydown', (event) => {
  if (event.key !== 'Escape') return;
  if (state.profileOpen) closeProfileModal();
  else if (state.vault.open) closeVaultPicker();
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

/* A página de arranque é a que o endereço pede, e não sempre o Resumo: um
   separador tem de poder ser guardado nos favoritos e partilhado, e abrir em
   `#cofre` e aterrar no Resumo tornaria o endereço mentira. */
applyRoute(currentRoute());

void boot();
