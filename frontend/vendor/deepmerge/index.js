'use strict';

// Stands in for the deepmerge package (the API of its 4.3.1 release), which the Svelte plugin of Vite uses to merge
// its options: that package has an advisory with no fixed release (GHSA-gcqx-w48m-w2h4, prototype poisoning of the
// merged object). package.json points every `deepmerge` of the dependency tree here through `overrides`.
//
// It merges as the original does, with one difference: a key `__proto__` (as JSON.parse produces it) is never copied.
// The original assigns it, which sets the prototype of the result when its value is an object and does nothing
// otherwise.
// deepmerge.test.mjs judges it against the original release, whose answers reference.json holds.

const REACT_ELEMENT = Symbol.for('react.element');
const { hasOwnProperty, propertyIsEnumerable, toString } = Object.prototype;

function isMergeableByDefault(value) {
  if (value === null || typeof value !== 'object') return false;
  const tag = toString.call(value);
  return tag !== '[object RegExp]' && tag !== '[object Date]' && value.$$typeof !== REACT_ELEMENT;
}

function cloneIfMergeable(value, options) {
  if (options.clone === false || !options.isMergeableObject(value)) return value;
  return deepmerge(Array.isArray(value) ? [] : {}, value, options);
}

function concatArrays(target, source, options) {
  return target.concat(source).map((item) => cloneIfMergeable(item, options));
}

// The keys copied from an object: `__proto__` is left out, the only key an assignment to a plain object turns into its
// prototype.
function ownEnumerableKeys(object) {
  const symbols = Object.getOwnPropertySymbols(object).filter((symbol) => propertyIsEnumerable.call(object, symbol));
  return Object.keys(object)
    .filter((key) => key !== '__proto__')
    .concat(symbols);
}

// `in` throws on a primitive target: such a target holds no property.
function hasProperty(object, key) {
  try {
    return key in object;
  } catch {
    return false;
  }
}

// A key the target only inherits or holds as a non-enumerable property (`toString`, `constructor` and `__proto__` on
// a plain object) is not taken from the source.
function isInheritedOnly(target, key) {
  return hasProperty(target, key) && !(hasOwnProperty.call(target, key) && propertyIsEnumerable.call(target, key));
}

function mergeFunction(key, options) {
  const custom = options.customMerge ? options.customMerge(key) : undefined;
  return typeof custom === 'function' ? custom : deepmerge;
}

function mergeObjects(target, source, options) {
  const result = {};
  if (options.isMergeableObject(target)) {
    for (const key of ownEnumerableKeys(target)) result[key] = cloneIfMergeable(target[key], options);
  }
  for (const key of ownEnumerableKeys(source)) {
    if (isInheritedOnly(target, key)) continue;
    if (hasProperty(target, key) && options.isMergeableObject(source[key])) {
      result[key] = mergeFunction(key, options)(target[key], source[key], options);
    } else {
      result[key] = cloneIfMergeable(source[key], options);
    }
  }
  return result;
}

function deepmerge(target, source, options) {
  options = options || {};
  options.arrayMerge = options.arrayMerge || concatArrays;
  options.isMergeableObject = options.isMergeableObject || isMergeableByDefault;
  // Given to a custom arrayMerge, as the original does.
  options.cloneUnlessOtherwiseSpecified = cloneIfMergeable;
  const sourceIsArray = Array.isArray(source);
  if (sourceIsArray !== Array.isArray(target)) return cloneIfMergeable(source, options);
  return sourceIsArray ? options.arrayMerge(target, source, options) : mergeObjects(target, source, options);
}

deepmerge.all = function all(objects, options) {
  if (!Array.isArray(objects)) throw new Error('first argument should be an array');
  return objects.reduce((merged, next) => deepmerge(merged, next, options), {});
};

module.exports = deepmerge;
