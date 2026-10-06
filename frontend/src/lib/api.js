// Client of the Mimicway management API. With authentication on, every request carries the user's access token, and a
// 401 signs the user out. In development, Vite's proxy forwards /api to http://localhost:7342.
//
// Requests go to /api/... on the origin that served the UI, unless /runtime-config.json gives another base URL
// (runtime-config.js): the infrastructure may route the API to another origin than the UI.
import { auth, logout } from './auth.svelte.js';
import { getApiBaseUrl } from './runtime-config.js';
import { t, getLocale } from './i18n.svelte.js';

const BASE = '/api';

async function request(method, path, body) {
  const opts = {
    method,
    // The server words its error messages in the language of the interface.
    headers: { 'Content-Type': 'application/json', 'Accept-Language': getLocale() },
  };
  if (auth.token) {
    opts.headers['Authorization'] = `Bearer ${auth.token}`;
  }
  if (body !== undefined) {
    opts.body = JSON.stringify(body);
  }
  const res = await fetch(`${getApiBaseUrl()}${BASE}${path}`, opts);
  if (res.status === 401 && auth.enabled) {
    logout();
    throw new Error(t('Session expired, please sign in again'));
  }
  if (!res.ok) {
    let msg = `${res.status} ${res.statusText}`;
    try {
      const body = await res.json();
      if (body.error) msg = body.error;
    } catch {}
    throw new Error(msg);
  }
  if (res.status === 204) return null;
  return res.json();
}

// Auth
export function getAuthStatus() {
  return request('GET', '/auth/status');
}

export function login(username, password) {
  return request('POST', '/auth/login', { username, password });
}

export function validateToken(token) {
  return request('POST', '/auth/validate', { token });
}

export function getMe() {
  return request('GET', '/auth/me');
}

// Services
//
// A service is identified by its group and its name together: two groups may each hold a service of the same name.
// Every service path is built by `servicePath`: `/groups/:group/services/:name...` for a service in a group,
// `/services/:name...` for an ungrouped one.
function servicePath(name, groupName, suffix = '') {
  return groupName
    ? `/groups/${encodeURIComponent(groupName)}/services/${encodeURIComponent(name)}${suffix}`
    : `/services/${encodeURIComponent(name)}${suffix}`;
}

export function getServices() {
  return request('GET', '/services');
}

export function getService(name, groupName = null) {
  return request('GET', servicePath(name, groupName));
}

export function createService(service) {
  return request('POST', '/services', service);
}

export function updateService(name, groupName, service) {
  return request('PUT', servicePath(name, groupName), service);
}

export function deleteService(name, groupName = null) {
  return request('DELETE', servicePath(name, groupName));
}

export function toggleService(name, groupName, isMocked) {
  return request('PUT', servicePath(name, groupName, '/toggle'), { is_mocked: isMocked });
}

export function pingService(name, groupName = null) {
  return request('POST', servicePath(name, groupName, '/ping'));
}

export function reorderRules(serviceName, groupName, order) {
  return request('PUT', servicePath(serviceName, groupName, '/rules/reorder'), { order });
}

// Observation of the proxied traffic of a service in proxy mode, turned on and off by the user, never automatically.
// getObservationStatus() lists every observed service the user can access (the server filters them); there is no
// status endpoint for a single service, the component filters the list.
export function observeService(name, groupName = null) {
  return request('POST', servicePath(name, groupName, '/observe'));
}

export function unobserveService(name, groupName = null) {
  return request('DELETE', servicePath(name, groupName, '/observe'));
}

export function getObservationStatus() {
  return request('GET', '/observation/status');
}

// Computed by the server at each call, nothing cached, from the exchanges captured while the service was observed (the
// latest few per endpoint, in memory only). The `outcome` of each entry is "Unconditional" (one rule, without
// condition), "Conditional" (one rule per value of the field that explains the differences) or "VarianceUnexplained"
// (a diagnosis only, nothing to suggest).
export function getServiceSuggestions(name, groupName = null) {
  return request('GET', servicePath(name, groupName, '/suggestions'));
}

// Config
export function getConfig() {
  return request('GET', '/config');
}

export function putConfig(config) {
  return request('PUT', '/config', config);
}

export function getLogs(limit = 50) {
  return request('GET', `/logs?limit=${limit}`);
}

// Rule tester: runs a draft rule, saved or not, against a request taken from the log, and changes nothing. The
// endpoint is stateless and loads no service, hence no servicePath().
export function testRule(payload) {
  return request('POST', '/rule-test', payload);
}

// Rule conflict detector, called when a rule is saved (RuleForm): compares the draft with the other rules of the
// service, sent in the payload, and reports obvious overlaps (same conditions, or conditions included in others). The
// user may save anyway, and a failed check saves without asking. Stateless like /rule-test, hence no servicePath().
export function checkRuleConflicts(payload) {
  return request('POST', '/rule-conflicts', payload);
}

// Messaging (Kafka). These routes answer 404 on a binary built without the "messaging-kafka" feature: callers handle
// that failure (App.svelte checks at startup).
export function getMessagingStatus() {
  return request('GET', '/messaging/status');
}

export function getMessagingLogs(limit = 200) {
  return request('GET', `/messaging/logs?limit=${limit}`);
}

export function simulateMessage(topic, payload, headers = {}) {
  return request('POST', '/messaging/simulate', { topic, payload, headers });
}

export function validateScript(script) {
  return request('POST', '/script/validate', { script });
}

// Raw TCP (binary protocols other than HTTP, mocks only, no proxy). These routes answer 404 on a binary built without
// the "tcp-mock" feature, handled like messaging (App.svelte). With authentication on, any signed-in user may read
// them; only super-admins may change the services.
export function getTcpStatus() {
  return request('GET', '/tcp/status');
}

export function getTcpServices() {
  return request('GET', '/tcp/services');
}

export function createTcpService(service) {
  return request('POST', '/tcp/services', service);
}

export function updateTcpService(name, service) {
  return request('PUT', `/tcp/services/${encodeURIComponent(name)}`, service);
}

export function deleteTcpService(name) {
  return request('DELETE', `/tcp/services/${encodeURIComponent(name)}`);
}

export function resetConfig() {
  return request('DELETE', '/config/reset');
}

export function getBackups() {
  return request('GET', '/config/backups');
}

export function restoreBackup(filename) {
  return request('POST', `/config/restore/${encodeURIComponent(filename)}`);
}

// Groups
export function getGroups() {
  return request('GET', '/groups');
}

export function createGroup(group) {
  return request('POST', '/groups', group);
}

export function updateGroup(name, group) {
  return request('PUT', `/groups/${encodeURIComponent(name)}`, group);
}

export function deleteGroup(name) {
  return request('DELETE', `/groups/${encodeURIComponent(name)}`);
}

export function updateGroupMembers(name, members) {
  return request('PUT', `/groups/${encodeURIComponent(name)}/members`, members);
}
