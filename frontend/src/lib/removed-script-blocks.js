// Earlier versions gave a rule two more script blocks, `pre_script` and `post_script`, run at the same moment as
// `script`. The server refuses a configuration where one of them still holds code (RuleInput in src/models/mod.rs),
// but in the words of an API error. An imported file is checked here first, so that the interface can name the rule
// and the block in the user's language before anything is sent.

const REMOVED_BLOCKS = ['pre_script', 'post_script'];

/**
 * The first rule of `config` whose removed script block holds code, as `{ service, rule, block }`, or null.
 * An empty or blank block is allowed: it never ran anything.
 */
export function findRemovedScriptBlock(config) {
  for (const service of config?.services ?? []) {
    for (const rule of service?.rules ?? []) {
      for (const block of REMOVED_BLOCKS) {
        const code = rule?.[block];
        if (typeof code === 'string' ? code.trim() !== '' : code != null) {
          return { service: String(service.name ?? ''), rule: String(rule.name ?? ''), block };
        }
      }
    }
  }
  return null;
}
