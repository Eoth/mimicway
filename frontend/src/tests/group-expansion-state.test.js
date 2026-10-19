import { describe, it, expect, beforeEach } from 'vitest';
import {
  isGroupExpanded,
  getExpandedGroupKeys,
  setGroupExpanded,
  toggleGroupExpanded,
  resetGroupExpansionState,
} from '../lib/group-expansion-state.svelte.js';

describe('group-expansion-state', () => {
  beforeEach(() => {
    resetGroupExpansionState();
  });

  it('starts with no group expanded', () => {
    expect(isGroupExpanded('team-a')).toBe(false);
    expect(getExpandedGroupKeys().size).toBe(0);
  });

  it('expands a group', () => {
    setGroupExpanded('team-a', true);
    expect(isGroupExpanded('team-a')).toBe(true);
  });

  it('collapses a group', () => {
    setGroupExpanded('team-a', true);
    setGroupExpanded('team-a', false);
    expect(isGroupExpanded('team-a')).toBe(false);
  });

  it('keeps the state of each group apart', () => {
    setGroupExpanded('team-a', true);
    setGroupExpanded('team-b', true);
    setGroupExpanded('team-a', false);

    expect(isGroupExpanded('team-a')).toBe(false);
    expect(isGroupExpanded('team-b')).toBe(true);
    expect(isGroupExpanded('team-c')).toBe(false);
    expect(getExpandedGroupKeys().size).toBe(1);
  });

  it('`toggleGroupExpanded` flips the current state', () => {
    expect(isGroupExpanded('team-a')).toBe(false);
    toggleGroupExpanded('team-a');
    expect(isGroupExpanded('team-a')).toBe(true);
    toggleGroupExpanded('team-a');
    expect(isGroupExpanded('team-a')).toBe(false);
  });

  it('`resetGroupExpansionState` collapses every group', () => {
    setGroupExpanded('team-a', true);
    setGroupExpanded('team-b', true);
    resetGroupExpansionState();
    expect(getExpandedGroupKeys().size).toBe(0);
    expect(isGroupExpanded('team-a')).toBe(false);
    expect(isGroupExpanded('team-b')).toBe(false);
  });
});
