// Runs once before any test of the suite: refuses a server that holds data someone keeps (disposable-server.mjs).
import { assertDisposableServer } from './disposable-server.mjs';

export default async function globalSetup(config) {
  await assertDisposableServer(config.projects[0].use.baseURL);
}
