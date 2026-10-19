import { describe, it, expect } from 'vitest';
import { extractPathParamNames, combinePathParamNames } from '../lib/path-params.js';

describe('extractPathParamNames', () => {
  it('extracts the names in braces', () => {
    expect(extractPathParamNames('/orders/{id}/items/{itemId}')).toEqual(['id', 'itemId']);
  });

  it('extracts the names in colon syntax', () => {
    expect(extractPathParamNames('/orders/:id/items/:itemId')).toEqual(['id', 'itemId']);
  });

  it('ignores the * wildcard', () => {
    expect(extractPathParamNames('/orders/*')).toEqual([]);
  });

  it('drops repeated names', () => {
    expect(extractPathParamNames('/a/{id}/b/{id}')).toEqual(['id']);
  });

  it('returns an empty array for a static pattern', () => {
    expect(extractPathParamNames('/orders/list')).toEqual([]);
  });

  it('returns an empty array for an empty or missing value', () => {
    expect(extractPathParamNames('')).toEqual([]);
    expect(extractPathParamNames(null)).toEqual([]);
    expect(extractPathParamNames(undefined)).toEqual([]);
  });
});

describe('combinePathParamNames', () => {
  it('combines several patterns without duplicates, in order of first appearance', () => {
    expect(combinePathParamNames(['/orders/{id}', '/{id}/sub/{subId}'])).toEqual(['id', 'subId']);
  });

  it('handles empty patterns mixed with valid ones', () => {
    expect(combinePathParamNames(['', '/users/{userId}'])).toEqual(['userId']);
  });

  it('returns an empty array when no pattern holds a parameter', () => {
    expect(combinePathParamNames(['/a/b', '/c/d'])).toEqual([]);
  });
});
