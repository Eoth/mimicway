import { describe, it, expect } from 'vitest';
import { RHAI_FUNCTIONS, filterRhaiFunctions, tokenAtCursor, computeInsertSelection } from '../lib/rhai-functions.js';

describe('RHAI_FUNCTIONS (source unique)', () => {
  it('chaque fonction a un nom, une signature, une description et un insertText', () => {
    for (const fn of RHAI_FUNCTIONS) {
      expect(fn.name).toBeTruthy();
      expect(fn.signature).toContain(fn.name);
      expect(fn.description.length).toBeGreaterThan(0);
      expect(fn.insertText).toContain(fn.name);
    }
  });

  it('les noms de fonctions sont uniques', () => {
    const names = RHAI_FUNCTIONS.map((f) => f.name);
    expect(new Set(names).size).toBe(names.length);
  });

  it('inclut les fonctions seedees et les fonctions de date generalisees', () => {
    const names = RHAI_FUNCTIONS.map((f) => f.name);
    expect(names).toContain('seeded_int');
    expect(names).toContain('seeded_pick');
    expect(names).toContain('date_now');
    expect(names).toContain('date_past');
    expect(names).toContain('date_future');
  });

  it("inclut parse_date, l'inverse de date_now/date_past/date_future", () => {
    const parseDate = RHAI_FUNCTIONS.find((f) => f.name === 'parse_date');
    expect(parseDate).toBeTruthy();
    expect(parseDate.signature).toBe('parse_date(text, "pattern")');
    expect(parseDate.description).toContain('milliseconds');
    expect(parseDate.description.toLowerCase()).toContain('error');
  });

  it('inclut les 4 accesseurs de contexte de requete (path/query/headers/body)', () => {
    const names = RHAI_FUNCTIONS.map((f) => f.name);
    expect(names).toContain('request.path');
    expect(names).toContain('request.query');
    expect(names).toContain('request.headers');
    expect(names).toContain('request.body');
  });

  it("documente que les noms d'en-tete sont normalises en minuscules", () => {
    const headers = RHAI_FUNCTIONS.find((f) => f.name === 'request.headers');
    expect(headers.description.toLowerCase()).toContain('lowercase');
  });
});

describe('filterRhaiFunctions', () => {
  it('retourne la liste complete pour une requete vide', () => {
    expect(filterRhaiFunctions('')).toEqual(RHAI_FUNCTIONS);
    expect(filterRhaiFunctions(undefined)).toEqual(RHAI_FUNCTIONS);
  });

  it('filtre par prefixe insensible a la casse', () => {
    const matches = filterRhaiFunctions('SEED');
    expect(matches.map((f) => f.name)).toEqual(['seeded_int', 'seeded_pick']);
  });

  it('ne retourne rien pour un prefixe inconnu', () => {
    expect(filterRhaiFunctions('zzz')).toEqual([]);
  });

  it('filtre exactement sur le prefixe, pas une sous-chaine quelconque', () => {
    // "now" est prefixe de now_ms/now_iso mais pas de year() ni uuid()
    const matches = filterRhaiFunctions('now');
    expect(matches.map((f) => f.name).sort()).toEqual(['now_iso', 'now_ms']);
  });

  it('taper "request" propose les 4 accesseurs de contexte (path/query/headers/body)', () => {
    const matches = filterRhaiFunctions('request');
    expect(matches.map((f) => f.name).sort()).toEqual([
      'request.body',
      'request.headers',
      'request.path',
      'request.query',
    ]);
  });
});

describe('tokenAtCursor', () => {
  it("extrait l'identifiant Rhai juste avant le curseur", () => {
    expect(tokenAtCursor('seed', 4)).toEqual({ token: 'seed', start: 0 });
  });

  it('retourne un token vide juste apres un espace ou une parenthese', () => {
    expect(tokenAtCursor('foo(', 4)).toEqual({ token: '', start: 4 });
    expect(tokenAtCursor('foo ', 4)).toEqual({ token: '', start: 4 });
  });

  it('ignore le texte apres le curseur', () => {
    expect(tokenAtCursor('rand_int', 4)).toEqual({ token: 'rand', start: 0 });
  });

  it("fonctionne au milieu d'un texte multi-lignes", () => {
    const text = 'let x = 1;\nseeded_pi';
    expect(tokenAtCursor(text, text.length)).toEqual({ token: 'seeded_pi', start: 11 });
  });
});

describe('computeInsertSelection', () => {
  it('selectionne les parametres entre parentheses', () => {
    expect(computeInsertSelection('random_int(min, max)')).toEqual({ start: 11, end: 19 });
  });

  it('place le curseur en fin de texte pour un appel sans parametre', () => {
    const sel = computeInsertSelection('now_ms()');
    expect(sel).toEqual({ start: 8, end: 8 });
  });

  it('place le curseur en fin de texte pour un accesseur de contexte (sans parentheses)', () => {
    const sel = computeInsertSelection('request.path');
    expect(sel).toEqual({ start: 'request.path'.length, end: 'request.path'.length });
  });
});
