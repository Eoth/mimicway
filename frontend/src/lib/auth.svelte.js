// Authentication state of the whole UI. With authentication off, nothing asks for a login. With it on, the access
// token lives in this module only, sent by api.js with every request: never in the browser's storage, where any script
// of the page's origin could read it, and gone with the page, so a reload asks to sign in again. The refresh token of
// the login answer is not kept: nothing renews a session, which ends when its access token expires.
export const auth = $state({
  enabled: false,
  // Whether the reset button shows when authentication is off; with it on, the button follows isSuperAdmin. Display
  // only: the server alone decides who may reset (require_super_admin).
  showResetButton: false,
  token: null,
  username: null,
  isSuperAdmin: false,
  groups: [],
});

export function isLoggedIn() {
  return !auth.enabled || auth.token !== null;
}

export function setAuth(data) {
  auth.token = data.access_token;
  auth.username = data.username;
  auth.isSuperAdmin = data.is_super_admin;
}

export function logout() {
  auth.token = null;
  auth.username = null;
  auth.isSuperAdmin = false;
  auth.groups = [];
}
