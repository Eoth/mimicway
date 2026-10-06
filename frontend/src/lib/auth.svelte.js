// Authentication state of the whole UI. With authentication off, nothing asks for a login. With it on, the access
// token is kept here, sent by api.js with every request and saved in localStorage so that a reload keeps the session.
export const auth = $state({
  enabled: false,
  // Whether the reset button shows when authentication is off; with it on, the button follows isSuperAdmin. Display
  // only: the server alone decides who may reset (require_super_admin).
  showResetButton: false,
  token: null,
  refreshToken: null,
  username: null,
  isSuperAdmin: false,
  groups: [],
});

export function isLoggedIn() {
  return !auth.enabled || auth.token !== null;
}

export function setAuth(data) {
  auth.token = data.access_token;
  auth.refreshToken = data.refresh_token || null;
  auth.username = data.username;
  auth.isSuperAdmin = data.is_super_admin;
  persistAuth();
}

export function logout() {
  auth.token = null;
  auth.refreshToken = null;
  auth.username = null;
  auth.isSuperAdmin = false;
  auth.groups = [];
  localStorage.removeItem('mimicway-auth');
}

export function persistAuth() {
  localStorage.setItem(
    'mimicway-auth',
    JSON.stringify({
      token: auth.token,
      refreshToken: auth.refreshToken,
      username: auth.username,
      isSuperAdmin: auth.isSuperAdmin,
    }),
  );
}

export function restoreAuth() {
  try {
    const saved = localStorage.getItem('mimicway-auth');
    if (saved) {
      const data = JSON.parse(saved);
      auth.token = data.token || null;
      auth.refreshToken = data.refreshToken || null;
      auth.username = data.username || null;
      auth.isSuperAdmin = data.isSuperAdmin || false;
    }
  } catch {
    // corrupted data, ignore
  }
}
