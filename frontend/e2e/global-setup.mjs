// Runs once before any test of the suite: refuses a server that holds data someone keeps (disposable-server.mjs), and
// accessibility exceptions whose date has passed or is too far (a11y-exceptions.mjs).
import { assertDisposableServer } from './disposable-server.mjs';
import { EXCEPTIONS, exceptionProblems } from './a11y-exceptions.mjs';

export default async function globalSetup(config) {
  await assertDisposableServer(config.projects[0].use.baseURL);
  const problems = exceptionProblems(EXCEPTIONS, new Date().toISOString().slice(0, 10));
  if (problems.length > 0) throw new Error(problems.join('\n'));
}
