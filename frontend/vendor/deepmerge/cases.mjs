// Cases on which the stand-in is judged against the original deepmerge: make-reference.mjs generates them and records
// the answers of the original in reference.json, deepmerge.test.js replays them on the stand-in.
//
// A case is JSON: the calling convention (VARIANTS) and the values to merge, described so that functions, dates,
// regular expressions, class instances, symbol keys and own `__proto__` keys survive the trip through JSON. An answer
// is encoded the same way, and says which parts of it are the very objects given as input (`ref`), which is how the
// merge clones or shares.
//
// The stand-in never copies a key `__proto__`: its expected answer is the original's on the same values without those
// keys (`withoutProto`), whose values are still built, so that every other object keeps its rank.

const SYMBOL = Symbol('s');
const SYMBOL_KEY = '@@s';

class Instance {}

function define(object, key, value) {
  Object.defineProperty(object, key, { value, enumerable: true, writable: true, configurable: true });
}

// Builds the values of a case; `ids` maps each object or function built to its rank, the identity encode() reports.
export function build(description, ids = new Map(), { withoutProto = false } = {}) {
  const register = (value) => {
    ids.set(value, ids.size);
    return value;
  };
  const fill = (target, entries) => {
    for (const [key, description] of entries) {
      const value = build(description, ids, { withoutProto });
      if (!(withoutProto && key === '__proto__')) define(target, key === SYMBOL_KEY ? SYMBOL : key, value);
    }
    return target;
  };
  if (description === null || typeof description !== 'object') return description;
  switch (description.$) {
    case 'undefined':
      return undefined;
    case 'nan':
      return NaN;
    case 'function':
      return register(function value() {});
    case 'date':
      return register(new Date(description.v));
    case 'regexp':
      return register(new RegExp(description.v));
    case 'map':
      return register(new Map([[1, 2]]));
    case 'react':
      return register({ $$typeof: Symbol.for('react.element'), type: 'div' });
    case 'array':
      return fill(
        register([]),
        description.v.map((value, index) => [String(index), value]),
      );
    case 'object':
      return fill(register({}), description.o);
    case 'instance':
      return fill(register(new Instance()), description.o);
    default:
      throw new Error(`unknown description ${description.$}`);
  }
}

function prototypeName(value) {
  const prototype = Object.getPrototypeOf(value);
  if (prototype === Object.prototype) return 'Object';
  if (prototype === Array.prototype) return 'Array';
  if (prototype === null) return 'null';
  return 'other';
}

// Encodes an answer; an object or function found in `ids` is one of the inputs and is encoded by its rank only.
export function encode(value, ids = new Map()) {
  if (value === undefined) return { $: 'undefined' };
  if (Number.isNaN(value)) return { $: 'nan' };
  if (typeof value === 'symbol') return { $: 'symbol', v: value.description };
  if (value === null || (typeof value !== 'object' && typeof value !== 'function')) return value;
  if (ids.has(value)) return { $: 'ref', id: ids.get(value) };
  if (typeof value === 'function') return { $: 'new function' };
  const entries = Reflect.ownKeys(value)
    .filter((key) => !(Array.isArray(value) && key === 'length'))
    .map((key) => [typeof key === 'symbol' ? SYMBOL_KEY : key, encode(value[key], ids)]);
  return { $: 'built', prototype: prototypeName(value), o: entries };
}

// Whether an encoded answer holds an object whose prototype is neither Object's nor Array's: the poisoning.
export function poisoned(encoded) {
  if (encoded === null || typeof encoded !== 'object') return false;
  if (encoded.$ !== 'built') return false;
  return !['Object', 'Array'].includes(encoded.prototype) || encoded.o.some(([, value]) => poisoned(value));
}

// The ways deepmerge is called. `plugin` is the call of @sveltejs/vite-plugin-svelte (mergeConfigs in
// src/utils/options.js), the only user of the package in this tree; the others cover the rest of its API.
export const VARIANTS = {
  plugin: (merge, values) => {
    let result = {};
    for (const config of values.filter((value) => value != null)) {
      result = merge(result, config, { arrayMerge: (target, source) => source ?? target });
    }
    return result;
  },
  defaults: (merge, [target, source]) => merge(target, source),
  all: (merge, values) => merge.all(values),
  notAnArray: (merge, [value]) => merge.all(value),
  noClone: (merge, [target, source]) => merge(target, source, { clone: false }),
  customMerge: (merge, [target, source]) =>
    merge(target, source, { customMerge: (key) => (key === 'a' ? (left, right) => [left, right] : undefined) }),
  plainOnly: (merge, [target, source]) =>
    merge(target, source, {
      isMergeableObject: (value) => Object.prototype.toString.call(value) === '[object Object]',
    }),
  customArrays: (merge, [target, source]) =>
    merge(target, source, {
      arrayMerge: (left, right, options) =>
        right.map((item, index) =>
          index in left ? merge(left[index], item, options) : options.cloneUnlessOtherwiseSpecified(item, options),
        ),
    }),
};

// Runs a case on one implementation and encodes its answer, or the error it threw.
export function run(merge, testCase, { withoutProto = false } = {}) {
  const ids = new Map();
  const values = testCase.values.map((description) => build(description, ids, { withoutProto }));
  try {
    return encode(VARIANTS[testCase.variant](merge, values), ids);
  } catch (error) {
    return { $: 'throws', name: error.name, message: error.message };
  }
}
